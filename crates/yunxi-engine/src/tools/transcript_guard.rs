//! Read-time privacy barrier for compact transcript carriers.
//!
//! Transcript files are intentionally retained on disk for rollback and
//! forensics, but a deleted memory must not become visible again merely
//! because a model asks `read`, `grep`, or `run_command` to open the file.
//! The check is carrier/provenance based: it never searches transcript text.

use super::{
    ToolCallContext, ToolRegistry, TranscriptAccessDecision, TranscriptAccessGuard,
    TranscriptReadCapability,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use yunxi_core::memory::MemoryStore;
use yunxi_core::state::StateStore;

const READ_TOOLS: &[&str] = &["read", "grep", "glob", "run_command"];
const FILE_ACCESS_COMMANDS: &[&str] = &[
    "cat", "head", "tail", "grep", "rg", "sed", "awk", "find", "ls", "stat", "readlink", "file",
    "cp", "mv", "rm", "touch", "tee", "cd",
];
const FILE_ACCESS_WRAPPERS: &[&str] = &[
    "busybox", "command", "doas", "env", "exec", "eval", "ionice", "nice", "nohup", "setsid",
    "sudo", "time", "timeout", "watch", "xargs",
];
const SHELL_COMMANDS: &[&str] = &["bash", "dash", "ksh", "sh", "zsh"];
const MAX_NESTED_SUBSTITUTION_DEPTH: usize = 8;

/// Bind the current session's transcript policy to a tool registry.
///
/// Static composition deliberately has no session or memory handles.  Binding
/// here keeps the provider-facing tool definitions byte-identical while making
/// the local execution path fail closed for legacy transcript carriers.
pub(crate) fn bind(registry: &mut ToolRegistry, state: StateStore, memory: MemoryStore) {
    let guard: TranscriptAccessGuard = Arc::new(move |tool, args| {
        if !READ_TOOLS.contains(&tool.name.as_str()) {
            return TranscriptAccessDecision::Allow;
        }
        let state = &state;
        let memory = &memory;
        match tool.name.as_str() {
            "read" => {
                let Some(path) = args
                    .get("path")
                    .and_then(Value::as_str)
                    .and_then(path_from_argument)
                else {
                    return TranscriptAccessDecision::Allow;
                };
                if let Some(reason) = check_path(state, memory, &path) {
                    return TranscriptAccessDecision::Deny(reason);
                }
                if has_live_provenance(state, memory, &path) {
                    return match open_transcript_capability(state, &path) {
                        Ok(capability) => TranscriptAccessDecision::AllowWith(
                            ToolCallContext::with_transcript(capability),
                        ),
                        Err(reason) => TranscriptAccessDecision::Deny(reason),
                    };
                }
                TranscriptAccessDecision::Allow
            }
            "grep" | "glob" => {
                let Some(path) = args
                    .get("path")
                    .and_then(Value::as_str)
                    .and_then(path_from_argument)
                else {
                    return TranscriptAccessDecision::Allow;
                };
                if let Some(reason) = check_path(state, memory, &path) {
                    return TranscriptAccessDecision::Deny(reason);
                }
                if has_live_provenance(state, memory, &path) {
                    if tool.name == "glob" {
                        return TranscriptAccessDecision::Deny(format!(
                            "transcript `{}` cannot be searched safely with glob; use read instead",
                            path.display()
                        ));
                    }
                    return match open_transcript_capability(state, &path) {
                        Ok(capability) => TranscriptAccessDecision::AllowWith(
                            ToolCallContext::with_transcript(capability),
                        ),
                        Err(reason) => TranscriptAccessDecision::Deny(reason),
                    };
                }
                TranscriptAccessDecision::Allow
            }
            // Shell parsing is intentionally not attempted. We block an exact
            // compact-session root mention, which is the absolute path emitted in
            // the transcript carrier hint, and we fail closed for opaque shell
            // expansion when it occurs in a file-access context. Structured reads
            // nested through `yunxi tool-call` hit the same registry barrier.
            "run_command" => args
                .get("command")
                .and_then(Value::as_str)
                .and_then(|command| check_command(state, memory, command))
                .map_or(
                    TranscriptAccessDecision::Allow,
                    TranscriptAccessDecision::Deny,
                ),
            _ => TranscriptAccessDecision::Allow,
        }
    });
    registry.bind_transcript_access_guard(guard);
}

fn path_from_argument(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("artifact:") || raw.starts_with("kb:") {
        return None;
    }
    Some(resolve_path(raw))
}

fn resolve_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home.join(rest);
        }
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&yunxi_base::workspace::effective_workdir().join(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn compact_root(state: &StateStore) -> PathBuf {
    normalize_path(&state.state_dir().join("compact"))
}

fn transcript_root(state: &StateStore) -> PathBuf {
    normalize_path(&compact_root(state).join(state.session_id().as_ref()))
}

fn is_transcript_scope(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn check_path(state: &StateStore, memory: &MemoryStore, path: &Path) -> Option<String> {
    let compact = compact_root(state);
    let path = if path.starts_with(&compact) {
        path.to_path_buf()
    } else {
        // A symlink alias outside `state/compact` must not turn a registered
        // transcript into an ordinary file. Canonicalize only as an alias
        // probe; ordinary paths that do not resolve remain outside this
        // transcript-specific barrier.
        let Ok(canonical) = std::fs::canonicalize(path) else {
            return None;
        };
        if !canonical.starts_with(&compact) {
            return None;
        }
        return Some(format!(
            "transcript `{}` is blocked because it uses a symlink alias",
            path.display()
        ));
    };
    let Some(relative) = path.strip_prefix(&compact).ok() else {
        return None;
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(session)) = components.next() else {
        return Some("compact transcript root is not a readable file".to_string());
    };
    let session = session.to_string_lossy();
    let current_session = state.session_id();
    if session != current_session.as_ref() {
        return Some(format!(
            "transcript belongs to another session `{session}` and is blocked"
        ));
    }
    let root = transcript_root(state);
    if !is_transcript_scope(&path, &root) {
        return Some("transcript path is outside the active session scope".to_string());
    }
    let path = path.to_string_lossy();
    match state
        .conv_db()
        .load_transcript_provenance_by_path(current_session.as_ref(), &path)
    {
        Ok(Some(provenance))
            if memory
                .memory_refs_are_tombstoned(&provenance.refs)
                .unwrap_or(true) =>
        {
            Some(format!(
                "transcript `{path}` is unavailable because its linked memory was deleted"
            ))
        }
        Ok(Some(_)) => ensure_transcript_identity(Path::new(path.as_ref()), &compact),
        Ok(None) => Some(format!(
            "transcript `{path}` has unknown provenance and is blocked"
        )),
        Err(_) => Some(format!(
            "transcript `{path}` provenance could not be verified and is blocked"
        )),
    }
}

fn has_live_provenance(state: &StateStore, memory: &MemoryStore, path: &Path) -> bool {
    let compact = compact_root(state);
    let session_root = transcript_root(state);
    if !path.starts_with(&compact) || !is_transcript_scope(path, &session_root) {
        return false;
    }
    let Ok(Some(provenance)) = state
        .conv_db()
        .load_transcript_provenance_by_path(state.session_id().as_ref(), &path.to_string_lossy())
    else {
        return false;
    };
    !memory
        .memory_refs_are_tombstoned(&provenance.refs)
        .unwrap_or(true)
}

fn open_transcript_capability(
    state: &StateStore,
    path: &Path,
) -> Result<TranscriptReadCapability, String> {
    let compact = compact_root(state);
    if let Some(reason) = ensure_transcript_identity(path, &compact) {
        return Err(reason);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| {
        format!(
            "transcript `{}` could not be opened for a protected read: {error}",
            path.display()
        )
    })?;
    Ok(TranscriptReadCapability::new(path.to_path_buf(), file))
}

/// Verify the filesystem identity immediately before opening a structured
/// transcript read.  The opened descriptor is then passed to the handler, so
/// a later leaf rename/replacement cannot change the bytes being read. Parent
/// replacement and arbitrary shell data-flow remain outside this narrow seam.
fn ensure_transcript_identity(path: &Path, compact: &Path) -> Option<String> {
    let mut ancestor = path;
    loop {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) => {
                return Some(format!(
                    "transcript `{}` identity could not be verified: {error}",
                    path.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Some(format!(
                "transcript `{}` is blocked because its path contains a symlink",
                path.display()
            ));
        }
        if ancestor == path {
            if !metadata.is_file() {
                return Some(format!(
                    "transcript `{}` is blocked because it is not a regular file",
                    path.display()
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Some(format!(
                        "transcript `{}` is blocked because its file identity has {} hard links",
                        path.display(),
                        metadata.nlink()
                    ));
                }
            }
        }
        if ancestor == compact {
            break;
        }
        let Some(parent) = ancestor.parent() else {
            break;
        };
        if parent == ancestor {
            break;
        }
        ancestor = parent;
    }
    None
}

fn check_command(state: &StateStore, memory: &MemoryStore, command: &str) -> Option<String> {
    let compact = compact_root(state);
    let compact_text = compact.to_string_lossy();
    // Shell grammar is intentionally not fully parsed. We inspect path-like
    // tokens and deny when one resolves inside this session's compact root.
    // This covers absolute paths emitted by compact extras and relative/`~/`
    // spellings that resolve to the same root. Opaque expansion is denied only
    // in a file-access context; unrelated substitutions remain executable.
    for token in command_path_tokens(command) {
        let candidate = resolve_path(token);
        if let Some(reason) = check_path(state, memory, &candidate) {
            return Some(reason);
        }
        if state
            .conv_db()
            .load_transcript_provenance_by_path(
                state.session_id().as_ref(),
                &candidate.to_string_lossy(),
            )
            .ok()
            .flatten()
            .is_some()
        {
            return Some(format!(
                "run_command cannot access protected transcript `{}`",
                candidate.display()
            ));
        }
    }
    // Preserve conservative behavior for a compact root embedded in shell
    // syntax that the lightweight tokenizer cannot split cleanly. We cannot
    // prove which session the expression resolves to, so deny it rather than
    // treating an opaque shell expression as a safe read.
    if command.contains(compact_text.as_ref()) {
        return Some("opaque compact transcript command is blocked".to_string());
    }
    if command_has_dynamic_file_access(command) {
        return Some("dynamic transcript path is blocked".to_string());
    }
    None
}

/// Return whether a command contains a file-access context without trying to
/// become a complete shell parser.  We inspect the first command in each
/// unquoted compound segment and unquoted redirection operators.  This keeps
/// harmless substitutions such as `echo "$(printf hi)"` available while
/// refusing to guess about dynamic paths used by file operations.
fn command_has_dynamic_file_access(command: &str) -> bool {
    if command_has_dynamic_wrapper_payload(command) {
        return true;
    }

    if command_segments(command).any(|segment| {
        let file_access = has_unquoted_redirection(segment) || segment_invokes_file_access(segment);
        file_access && has_dynamic_shell_syntax(segment)
    }) {
        return true;
    }

    if command.is_empty() {
        return false;
    }

    nested_shell_fragments(command)
        .into_iter()
        .any(|fragment| command_has_dynamic_file_access_at_depth(&fragment, 1))
}

fn command_has_dynamic_file_access_at_depth(command: &str, depth: usize) -> bool {
    if command_has_dynamic_wrapper_payload(command) {
        return true;
    }

    if command_segments(command).any(|segment| {
        let file_access = has_unquoted_redirection(segment) || segment_invokes_file_access(segment);
        file_access && has_dynamic_shell_syntax(segment)
    }) {
        return true;
    }

    if depth >= MAX_NESTED_SUBSTITUTION_DEPTH {
        // Do not silently treat an unscanned deeply nested substitution as
        // safe. The cap keeps this lexical guard bounded while preserving its
        // fail-closed behavior for pathological shell input.
        return !nested_shell_fragments(command).is_empty();
    }

    nested_shell_fragments(command)
        .into_iter()
        .any(|fragment| command_has_dynamic_file_access_at_depth(&fragment, depth + 1))
}

/// Inspect shell and command wrappers without attempting to parse the full
/// shell grammar. A wrapper's quoted payload is a separate command string, so
/// scanning only the outer segment would hide dynamic paths behind quotes.
/// Dynamic or otherwise opaque payloads are rejected conservatively; a static
/// payload is recursively scanned for the same wrapper/file-access patterns.
fn command_has_dynamic_wrapper_payload(command: &str) -> bool {
    command_segments(command).any(segment_has_dynamic_wrapper_payload)
}

fn segment_has_dynamic_wrapper_payload(segment: &str) -> bool {
    let tokens = shell_tokens(segment);
    let Some(first_index) = tokens
        .iter()
        .position(|token| !is_shell_assignment(&token.text))
    else {
        return false;
    };
    let first = command_basename(&tokens[first_index].text);

    if SHELL_COMMANDS.contains(&first) {
        let mut index = first_index + 1;
        while let Some(token) = tokens.get(index) {
            if is_shell_command_option(&token.text) {
                if let Some(payload) = token.text.strip_prefix("--command=") {
                    let payload = std::iter::once(payload)
                        .chain(
                            tokens
                                .iter()
                                .skip(index + 1)
                                .map(|token| token.text.as_str()),
                        )
                        .collect::<Vec<_>>()
                        .join(" ");
                    return wrapper_payload_is_dynamic(&payload);
                }
                let Some(payload) = tokens.get(index + 1) else {
                    return true;
                };
                return wrapper_payload_is_dynamic(&payload.text);
            }
            index += 1;
        }
        return false;
    }

    if FILE_ACCESS_WRAPPERS.contains(&first) {
        let payload = tokens
            .iter()
            .skip(first_index + 1)
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return !payload.is_empty() && wrapper_payload_is_dynamic(&payload);
    }

    false
}

fn is_shell_command_option(token: &str) -> bool {
    token == "-c"
        || token == "--command"
        || token.starts_with("--command=")
        || is_combined_shell_c_option(token)
}

fn is_combined_shell_c_option(token: &str) -> bool {
    token.starts_with('-')
        && !token.starts_with("--")
        && token.len() > 2
        && token
            .as_bytes()
            .get(1..)
            .is_some_and(|bytes| bytes.contains(&b'c'))
}

fn wrapper_payload_is_dynamic(payload: &str) -> bool {
    if has_dynamic_shell_syntax(payload) {
        return true;
    }
    command_has_dynamic_file_access(payload)
}

#[derive(Debug)]
struct ShellToken {
    text: String,
}

/// Tokenize just enough shell syntax to retain quoted wrapper payloads. Quote
/// delimiters are removed, while the contents (including nested shell quotes)
/// remain available to the recursive lexical scan.
fn shell_tokens(command: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut token_started = false;

    for ch in command.chars() {
        if escaped {
            token.push(ch);
            escaped = false;
            token_started = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            token.push(ch);
            escaped = true;
            token_started = true;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    token.push(ch);
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                } else {
                    token.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    token_started = true;
                }
                ch if ch.is_whitespace() => {
                    if token_started {
                        tokens.push(ShellToken {
                            text: std::mem::take(&mut token),
                        });
                        token_started = false;
                    }
                }
                _ => {
                    token.push(ch);
                    token_started = true;
                }
            },
            _ => unreachable!(),
        }
    }

    if token_started {
        tokens.push(ShellToken { text: token });
    }
    tokens
}

/// Extract the bodies of the first shell-level `$()` and backtick
/// substitutions without attempting to parse the complete shell grammar.
///
/// The scanner skips single-quoted text, respects escapes, balances nested
/// parentheses in `$()` and stops after a bounded recursion depth in the
/// caller. This is deliberately a lexical helper for the transcript barrier,
/// not a general-purpose shell parser.
fn nested_shell_fragments(command: &str) -> Vec<String> {
    let chars = command.chars().collect::<Vec<_>>();
    let mut fragments = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            index += 1;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
                index += 1;
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                    index += 1;
                } else if ch == '$' && chars.get(index + 1) == Some(&'(') {
                    if let Some((fragment, end)) = extract_dollar_paren(&chars, index) {
                        fragments.push(fragment);
                        index = end;
                    } else {
                        index += 1;
                    }
                } else if ch == '`' {
                    if let Some((fragment, end)) = extract_backticks(&chars, index) {
                        fragments.push(fragment);
                        index = end;
                    } else {
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    index += 1;
                } else if ch == '$' && chars.get(index + 1) == Some(&'(') {
                    if let Some((fragment, end)) = extract_dollar_paren(&chars, index) {
                        fragments.push(fragment);
                        index = end;
                    } else {
                        index += 1;
                    }
                } else if ch == '`' {
                    if let Some((fragment, end)) = extract_backticks(&chars, index) {
                        fragments.push(fragment);
                        index = end;
                    } else {
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            _ => unreachable!(),
        }
    }
    fragments
}

fn extract_dollar_paren(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut depth = 1usize;
    let mut quote = None;
    let mut escaped = false;
    let mut index = start + 2;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            index += 1;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                } else if ch == '$' && chars.get(index + 1) == Some(&'(') {
                    depth += 1;
                    index += 1;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((chars[start + 2..index].iter().collect(), index + 1));
                    }
                }
                _ => {}
            },
            _ => unreachable!(),
        }
        index += 1;
    }
    None
}

fn extract_backticks(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut escaped = false;
    let mut index = start + 1;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == '`' {
            return Some((chars[start + 1..index].iter().collect(), index + 1));
        }
        index += 1;
    }
    None
}

fn command_segments(command: &str) -> impl Iterator<Item = &str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in command.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                ';' | '|' | '&' | '\n' => {
                    segments.push(&command[start..index]);
                    start = index + ch.len_utf8();
                }
                _ => {}
            },
            _ => unreachable!(),
        }
    }
    segments.push(&command[start..]);
    segments.into_iter()
}

fn segment_invokes_file_access(segment: &str) -> bool {
    let tokens = command_path_tokens(segment).collect::<Vec<_>>();
    let Some(first_index) = tokens.iter().position(|token| !is_shell_assignment(token)) else {
        return false;
    };
    let first = command_basename(tokens[first_index]);
    if FILE_ACCESS_COMMANDS.contains(&first) {
        return true;
    }
    if FILE_ACCESS_WRAPPERS.contains(&first) {
        return tokens
            .iter()
            .skip(first_index + 1)
            .map(|token| command_basename(token))
            .any(|token| FILE_ACCESS_COMMANDS.contains(&token));
    }
    if SHELL_COMMANDS.contains(&first) {
        let mut after_c = false;
        return tokens.iter().skip(first_index + 1).any(|token| {
            if after_c {
                return FILE_ACCESS_COMMANDS.contains(&command_basename(token));
            }
            if *token == "-c" || token.starts_with("-c") {
                after_c = true;
            }
            false
        });
    }
    false
}

fn command_basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

fn is_shell_assignment(token: &str) -> bool {
    let Some((raw_name, _)) = token.split_once('=') else {
        return false;
    };
    let name = raw_name.strip_suffix('+').unwrap_or(raw_name);
    !name.is_empty()
        && name.chars().enumerate().all(|(index, ch)| {
            (index == 0 && (ch == '_' || ch.is_ascii_alphabetic()))
                || (index > 0 && (ch == '_' || ch.is_ascii_alphanumeric()))
        })
}

fn has_unquoted_redirection(segment: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for ch in segment.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '<' | '>' => return true,
                _ => {}
            },
            _ => unreachable!(),
        }
    }
    false
}

fn has_dynamic_shell_syntax(command: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let chars = command.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            index += 1;
            continue;
        }
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                } else if ch == '$' && is_dynamic_dollar(&chars, index) {
                    return true;
                } else if ch == '`' {
                    return true;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '`' => return true,
                '$' if is_dynamic_dollar(&chars, index) => return true,
                _ => {}
            },
            _ => unreachable!(),
        }
        index += 1;
    }
    false
}

fn is_dynamic_dollar(chars: &[char], index: usize) -> bool {
    let Some(next) = chars.get(index + 1).copied() else {
        return false;
    };
    next == '('
        || next == '{'
        || next == '_'
        || next.is_ascii_alphabetic()
        || next.is_ascii_digit()
        || matches!(next, '@' | '*' | '?' | '#' | '!' | '-')
}

fn command_path_tokens(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '`' | ';' | '|' | '&'))
        .map(|token| token.trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}')))
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_scope_does_not_match_sibling_sessions() {
        let root = Path::new("/state/compact/session-a");
        assert!(is_transcript_scope(root, root));
        assert!(is_transcript_scope(&root.join("fold-1.md"), root));
        assert!(!is_transcript_scope(
            Path::new("/state/compact/session-ab"),
            root
        ));
    }

    #[test]
    fn command_probe_only_uses_exact_root_prefix() {
        assert!("cat /state/compact/s/fold-1.md".contains("/state/compact/s"));
        assert!(!"cat /state/compact/other/fold-1.md".contains("/state/compact/s"));
    }

    #[test]
    fn command_path_tokens_keep_relative_and_quoted_paths() {
        let tokens =
            command_path_tokens("cat './state/compact/session/fold-1.md' && printf \"done\"")
                .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec!["cat", "./state/compact/session/fold-1.md", "printf", "done"]
        );
    }

    #[tokio::test]
    async fn unknown_transcript_path_is_denied_before_handler() {
        let temp = tempfile::tempdir().unwrap();
        let paths = yunxi_base::paths::YunXiPaths {
            root_dir: temp.path().to_path_buf(),
            config_dir: temp.path().join("config"),
            config_file: temp.path().join("config/config.jsonc"),
            skills_dir: temp.path().join("config/skills"),
            data_dir: temp.path().join("data"),
            cache_dir: temp.path().join("cache"),
            state_dir: temp.path().join("state"),
            pictures_dir: temp.path().join("pictures"),
            fish_hook_file: temp.path().join("config/fish/conf.d/yunxi.fish"),
            bash_hook_file: temp.path().join("config/shell/bash-hook.sh"),
            zsh_hook_file: temp.path().join("config/shell/zsh-hook.zsh"),
            scripts_dir: temp.path().join("config/scripts"),
            system_scripts_dir: temp.path().join("system-scripts"),
        };
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let root = transcript_root(&state);
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "read",
            "read",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        // Exercise the public execution-registry facade used by hosts and the
        // direct CLI, rather than the module-private implementation.
        super::super::bind_transcript_access_guard(&mut registry, state, memory);
        let path = root
            .join("../")
            .join(root.file_name().unwrap())
            .join("fold-1.md");
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn registered_transcript_without_dead_refs_is_readable() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let result = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap();
        assert!(result.contains("handler ran"));
        drop(temp);
    }

    #[tokio::test]
    async fn opened_transcript_capability_survives_path_replacement() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "original").unwrap();
        let replacement = path.with_extension("replacement");
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new_with_context(
            "read",
            "read",
            serde_json::json!({"type":"object"}),
            move |args, _progress, context| {
                let replacement = replacement.clone();
                async move {
                    let path = PathBuf::from(args.get("path").unwrap().as_str().unwrap());
                    std::fs::rename(&path, &replacement).unwrap();
                    std::fs::write(&path, "replacement").unwrap();
                    let mut file = context
                        .transcript_for(&path)
                        .ok_or_else(|| anyhow::anyhow!("missing transcript capability"))?;
                    let mut content = String::new();
                    std::io::Read::read_to_string(&mut file, &mut content)?;
                    Ok(content)
                }
            },
        ));
        bind(&mut registry, state, memory);
        let result = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap();
        assert_eq!(result, "original");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        drop(temp);
    }

    #[tokio::test]
    async fn live_registered_transcript_search_allows_grep_but_denies_glob() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let mut registry = search_registry();
        bind(&mut registry, state, memory);
        let grep = registry
            .call(
                "grep",
                &serde_json::json!({"path": path, "pattern": "safe"}).to_string(),
            )
            .await
            .unwrap();
        assert_eq!(grep, "handler ran");
        let error = registry
            .call(
                "glob",
                &serde_json::json!({"path": path, "pattern": "safe"}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be searched safely"), "{error}");
        drop(temp);
    }

    #[tokio::test]
    async fn live_registered_transcript_is_denied_to_run_command() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": format!("cat '{}'", path.display())}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("run_command cannot access protected transcript"),
            "unexpected: {error}"
        );
        drop(temp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transcript_leaf_symlink_replacement_is_denied() {
        use std::os::unix::fs::symlink;

        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = temp.path().join("outside.md");
        std::fs::write(&outside, "outside").unwrap();
        symlink(&outside, &path).unwrap();
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("contains a symlink"), "unexpected: {error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transcript_symlink_alias_outside_compact_is_denied() {
        use std::os::unix::fs::symlink;

        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let alias = temp.path().join("transcript-alias.md");
        symlink(&path, &alias).unwrap();
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": alias}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("symlink alias") || error.contains("identity"),
            "unexpected: {error}"
        );
        drop(temp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transcript_compact_root_symlink_is_denied() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let path = register_transcript(&state, &[]);
        let compact = compact_root(&state);
        if let Ok(metadata) = std::fs::symlink_metadata(&compact) {
            if metadata.file_type().is_dir() {
                std::fs::remove_dir_all(&compact).unwrap();
            } else {
                std::fs::remove_file(&compact).unwrap();
            }
        }
        let real_compact = temp.path().join("real-compact");
        std::fs::create_dir_all(&real_compact).unwrap();
        symlink(&real_compact, &compact).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("contains a symlink"), "unexpected: {error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transcript_hardlink_replacement_is_denied() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = temp.path().join("outside.md");
        std::fs::write(&outside, "outside").unwrap();
        std::fs::hard_link(&outside, &path).unwrap();
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("hard links"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn ordinary_file_command_remains_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);
        let ordinary = temp.path().join("ordinary.txt");
        std::fs::write(&ordinary, "ordinary").unwrap();
        let result = registry
            .call(
                "run_command",
                &serde_json::json!({"command": format!("cat '{}'", ordinary.display())})
                    .to_string(),
            )
            .await
            .unwrap();
        assert_eq!(result, "handler ran");
    }

    #[tokio::test]
    async fn registered_transcript_with_tombstoned_ref_is_denied() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let config = yunxi_base::config::AppConfig::default();
        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory.init().unwrap();
        let fact_id = memory.remember_fact("private", "test").unwrap();
        memory
            .delete_item(yunxi_core::memory::browse::BrowseTable::Facts, fact_id)
            .unwrap();
        let path = register_transcript(
            &state,
            &[yunxi_base::memory_types::MemoryRef {
                kind: "fact".to_string(),
                id: fact_id,
            }],
        );
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory was deleted"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn run_command_shell_chain_cannot_read_tombstoned_transcript() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let config = yunxi_base::config::AppConfig::default();
        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory.init().unwrap();
        let fact_id = memory.remember_fact("private", "test").unwrap();
        memory
            .delete_item(yunxi_core::memory::browse::BrowseTable::Facts, fact_id)
            .unwrap();
        let path = register_transcript(
            &state,
            &[yunxi_base::memory_types::MemoryRef {
                kind: "fact".to_string(),
                id: fact_id,
            }],
        );
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);
        let command = format!("cat '{}' ; printf done", path.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory was deleted"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn historical_session_transcript_is_denied_even_with_live_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let current_session = state.session_id().to_string();
        let historical = state
            .create_session(
                "yunxi",
                "historical",
                yunxi_core::state::USER_SESSION_KIND,
                None,
            )
            .unwrap();
        state.switch_session(&historical.session_id).unwrap();
        let path = register_transcript(&state, &[]);
        state.switch_session(&current_session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "historical").unwrap();

        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("another session"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn literal_glob_inside_compact_scope_is_denied_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let root = transcript_root(&state);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);

        let command = format!("cat '{}/fold-*.md'", root.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn dynamic_assignment_transcript_path_is_denied_before_handler() {
        let error = call_guarded_run_command("p=$STATE_COMPACT; cat \"$p/fold.md\"")
            .await
            .unwrap_err();
        assert!(
            error.contains("dynamic transcript path"),
            "unexpected: {error}"
        );
    }

    #[tokio::test]
    async fn command_substitution_transcript_path_is_denied_before_handler() {
        let error = call_guarded_run_command("cat \"$(printf '%s' \"$STATE_COMPACT\")\"")
            .await
            .unwrap_err();
        assert!(
            error.contains("dynamic transcript path"),
            "unexpected: {error}"
        );
    }

    #[tokio::test]
    async fn cd_then_dynamic_relative_read_is_denied_before_handler() {
        let error = call_guarded_run_command("cd /tmp; cat \"$name\"")
            .await
            .unwrap_err();
        assert!(
            error.contains("dynamic transcript path"),
            "unexpected: {error}"
        );
    }

    #[tokio::test]
    async fn dynamic_redirect_is_denied_but_echo_substitution_is_allowed() {
        let error = call_guarded_run_command("printf x > \"$p/fold\"")
            .await
            .unwrap_err();
        assert!(
            error.contains("dynamic transcript path"),
            "unexpected: {error}"
        );

        let result = call_guarded_run_command(r#"echo "$(printf hi)""#)
            .await
            .expect("echo without file access should reach the handler");
        assert_eq!(result, "handler ran");

        let result = call_guarded_run_command("cd /tmp; echo \"$name\"")
            .await
            .expect("an unrelated dynamic echo segment should reach the handler");
        assert_eq!(result, "handler ran");
    }

    #[tokio::test]
    async fn dynamic_file_access_aliases_and_shell_parameters_are_denied() {
        for command in [
            "/bin/cat \"$p/fold.md\"",
            "./cat \"$p/fold.md\"",
            "sudo cat \"$p/fold.md\"",
            "A+=v cat \"$p/fold.md\"",
            "cat \"$1\"",
        ] {
            let error = call_guarded_run_command(command).await.unwrap_err();
            assert!(
                error.contains("dynamic transcript path"),
                "command {command:?}: unexpected error: {error}"
            );
        }
    }

    #[tokio::test]
    async fn nested_file_access_is_denied_but_nested_printf_is_allowed() {
        for command in [
            r#"echo "$(cat "$p/fold.md")""#,
            r#"echo `cat "$p/fold.md"`"#,
        ] {
            let error = call_guarded_run_command(command).await.unwrap_err();
            assert!(
                error.contains("dynamic transcript path"),
                "command {command:?}: unexpected error: {error}"
            );
        }

        for command in [r#"echo "$(printf hi)""#, r#"echo `printf hi`"#] {
            let result = call_guarded_run_command(command)
                .await
                .expect("nested non-file substitution should reach the handler");
            assert_eq!(result, "handler ran", "command {command:?}");
        }
    }

    #[tokio::test]
    async fn shell_wrapper_payloads_are_scanned_recursively() {
        for command in [
            r#"sh -c 'cat "$p/fold.md"'"#,
            r#"bash -lc 'cat "$p/fold.md"'"#,
            r#"sh --command='cat "$p/fold.md"'"#,
            r#"eval 'cat "$p/fold.md"'"#,
            r#"sh -c "$cmd""#,
            r#"echo "$(sh -c 'cat "$p"')""#,
        ] {
            let error = call_guarded_run_command(command).await.unwrap_err();
            assert!(
                error.contains("dynamic transcript path"),
                "command {command:?}: unexpected error: {error}"
            );
        }

        for command in [r#"sh -c 'printf hi'"#, r#"eval 'printf hi'"#] {
            let result = call_guarded_run_command(command)
                .await
                .expect("static wrapper payload without file access should reach the handler");
            assert_eq!(result, "handler ran", "command {command:?}");
        }
    }

    #[tokio::test]
    async fn cd_into_compact_scope_is_denied_before_relative_read() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let root = transcript_root(&state);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);

        let command = format!("cd '{}'; cat fold-1.md", root.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    fn read_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "read",
            "read",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        registry
    }

    fn run_command_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "run_command",
            "run command",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        registry
    }

    fn search_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        for name in ["grep", "glob"] {
            registry.register(super::super::ToolSpec::new(
                name,
                name,
                serde_json::json!({"type":"object"}),
                |_| async { Ok("handler ran".to_string()) },
            ));
        }
        registry
    }

    async fn call_guarded_run_command(command: &str) -> Result<String, String> {
        let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
        let paths = test_paths(temp.path());
        let state =
            yunxi_core::state::StateStore::new(&paths).map_err(|error| error.to_string())?;
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);
        registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .map_err(|error| error.to_string())
    }

    fn transcript_fixture(
        refs: &[yunxi_base::memory_types::MemoryRef],
    ) -> (
        tempfile::TempDir,
        yunxi_core::state::StateStore,
        yunxi_core::memory::MemoryStore,
        PathBuf,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let path = register_transcript(&state, refs);
        (temp, state, memory, path)
    }

    fn register_transcript(
        state: &yunxi_core::state::StateStore,
        refs: &[yunxi_base::memory_types::MemoryRef],
    ) -> PathBuf {
        state.start_turn("t1", "hello", 1).unwrap();
        state.complete_turn("t1", "reply", None).unwrap();
        let visible = state.load_visible_turns().unwrap();
        let turn_ids = visible
            .iter()
            .map(|turn| turn.turn_id.clone())
            .collect::<Vec<_>>();
        let path = transcript_root(state).join("fold-1.md");
        state
            .replace_visible_with_summary_with_refs_and_transcripts(
                &turn_ids,
                &turn_ids,
                "summary",
                Default::default(),
                false,
                None,
                None,
                refs,
                &[yunxi_core::state::TranscriptCarrier {
                    transcript_id: "transcript-test".to_string(),
                    path: path.display().to_string(),
                }],
            )
            .unwrap();
        path
    }

    fn test_paths(root: &Path) -> yunxi_base::paths::YunXiPaths {
        yunxi_base::paths::YunXiPaths {
            root_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            config_file: root.join("config/config.jsonc"),
            skills_dir: root.join("config/skills"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            pictures_dir: root.join("pictures"),
            fish_hook_file: root.join("config/fish/conf.d/yunxi.fish"),
            bash_hook_file: root.join("config/shell/bash-hook.sh"),
            zsh_hook_file: root.join("config/shell/zsh-hook.zsh"),
            scripts_dir: root.join("config/scripts"),
            system_scripts_dir: root.join("system-scripts"),
        }
    }
}
