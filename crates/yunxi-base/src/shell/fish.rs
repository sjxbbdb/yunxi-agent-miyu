use crate::i18n::text as t;
use crate::paths::YunXiPaths;
use anyhow::Result;

fn completion_entries() -> [(&'static str, &'static str); 16] {
    [
        (
            "ask",
            t("Send a message to the assistant", "向助手发送一条消息"),
        ),
        (
            "init",
            t(
                "Create default configuration and state files; use <shell>-init for shell hooks",
                "创建默认配置和状态文件；Shell 集成请使用对应的 <shell>-init 命令",
            ),
        ),
        (
            "paths",
            t("Show application paths", "显示应用配置、数据和缓存路径"),
        ),
        (
            "config",
            t("Open or manage configuration", "打开或管理配置"),
        ),
        (
            "reload",
            t(
                "Reload configuration in the running YunXi daemon",
                "在运行中的 YunXi daemon 内重新加载配置",
            ),
        ),
        ("models", t("List or switch models", "列出或切换模型")),
        (
            "fish-init",
            t(
                "Integrate with fish for natural-language terminal conversations",
                "集成到 fish，集成后可在终端直接使用自然语言交流。",
            ),
        ),
        (
            "bash-init",
            t(
                "Integrate with bash for natural-language terminal conversations",
                "集成到 bash，集成后可在终端直接使用自然语言交流。",
            ),
        ),
        (
            "zsh-init",
            t(
                "Integrate with zsh for natural-language terminal conversations",
                "集成到 zsh，集成后可在终端直接使用自然语言交流。",
            ),
        ),
        (
            "remove-shell-hook",
            t(
                "Remove installed YunXi shell hooks",
                "安全删除已安装的 YunXi shell hook",
            ),
        ),
        ("history", t("Show conversation history", "显示会话历史")),
        ("kb", t("Manage the local knowledge base", "管理本地知识库")),
        (
            "update-default-kb",
            t("Update the default knowledge base", "更新 YunXi 默认知识库"),
        ),
        (
            "memory",
            t("Inspect or edit assistant memory", "查看或编辑助手记忆"),
        ),
        ("skills", t("Manage assistant skills", "管理助手 skills")),
        (
            "reset",
            t("Clear current conversation history", "清空当前会话历史"),
        ),
    ]
}

pub fn hook() -> String {
    super::stamp_hook("fish-init", &body())
}

fn body() -> String {
    let mut output = super::locate::fish_prelude();
    for (command, description) in completion_entries() {
        output.push_str(&format!(
            "complete -c yunxi -n __fish_use_subcommand -f -a {command} -d '{description}'\n"
        ));
    }
    output.push('\n');
    output.push_str(
        r#"function __yunxi_paste
    set -l output (yunxi --clipboard-paste 2>/dev/null)
    if test $status -eq 0; and test -n "$output"
        if not set -q __yunxi_image_counter
            set -g __yunxi_image_counter 0
        end
        set __yunxi_image_counter (math $__yunxi_image_counter + 1)
        # 视频的占位符标签是 Video,只替 Image 的话第二个视频起序号永远是 1,
        # 解析端会把它们都当成第一个附件(08-28)。
        set output (string replace -r '^\[(Image|Video) 1' "[\$1 $__yunxi_image_counter" -- $output)
        commandline -i -- $output
        commandline -f repaint
    else
        fish_clipboard_paste
    end
end

bind \cv __yunxi_paste

function __yunxi_insert_newline
    commandline -f expand-abbr
    commandline -i \n
end

bind ctrl-j __yunxi_insert_newline
bind \cj __yunxi_insert_newline
bind -M insert ctrl-j __yunxi_insert_newline
bind -M insert \cj __yunxi_insert_newline

function __yunxi_wrap_fish_prompt
    functions -q __yunxi_original_fish_prompt; and return
    functions -q fish_prompt; or fish_prompt >/dev/null 2>/dev/null
    functions -q fish_prompt; or return

    functions -c fish_prompt __yunxi_original_fish_prompt
    function fish_prompt
        if set -q __yunxi_pending_buffer
            printf '\e[?25l'
        end
        __yunxi_original_fish_prompt
    end
end

function __yunxi_replay_buffer
    set -l buffer $argv[1]
    set -l lines (string split \n -- "$buffer")
    if test (count $lines) -gt 0
        set -l prompt (fish_prompt | string collect -N)
        set -l prompt_lines (string split \n -- "$prompt")
        set -l prompt_col (math (string length --visible -- "$prompt_lines[-1]") + 1)
        printf '\e[?25l'
        printf '\e[1A\e[%sG' $prompt_col
        if not set -q fish_color_error; or not set_color $fish_color_error 2>/dev/null
            set_color red
        end
        printf '%s\n' "$lines[1]"
        for line in $lines[2..-1]
            printf '  %s\n' "$line"
        end
        set_color normal
    end
end

function __yunxi_restore_cursor
    printf '\e[?25h'
    set -e __yunxi_cursor_hidden
end

function __yunxi_on_prompt --on-event fish_prompt
    set -q __yunxi_pending_buffer; or return

    set -l buffer $__yunxi_pending_buffer
    set -e __yunxi_pending_buffer
    set -e __yunxi_image_counter

    trap __yunxi_restore_cursor INT TERM EXIT
    __yunxi_replay_buffer "$buffer"
    printf '\n'
    printf '%s' "$buffer" | yunxi --shell-intercept --shell fish --stdin
    set -l yunxi_status $status
    trap - INT TERM EXIT
    __yunxi_restore_cursor
    return $yunxi_status
end

function __yunxi_execute_or_continue
    commandline --is-valid
    set -l valid_status $status
    if test $valid_status -eq 2
        commandline -i \n
        commandline -f repaint
    else
        set -e __yunxi_image_counter
        commandline -f execute
    end
end

function __yunxi_buffer_is_multiline
    test (string split \n -- "$argv[1]" | count) -gt 1
end

function __yunxi_first_command
    set -l tokens (commandline --input="$argv[1]" --tokens-expanded 2>/dev/null)
    while test (count $tokens) -gt 0
        set -l token $tokens[1]
        if string match -qr '^[A-Za-z_][A-Za-z0-9_]*=' -- "$token"
            set -e tokens[1]
            continue
        end
        printf '%s' "$token"
        return 0
    end
    return 1
end

# 只看首词长什么样,所以取未展开的原文:--tokens-expanded 会真的跑命令替换,
# 放在每次回车上按不得。fish 的 commandline 没有稳定的 --tokens-raw 公共选项;
# 用当前 process + tokenize 只做语法分词,不执行通配符/命令替换。
function __yunxi_first_token_raw
    set -l tokens (commandline --input="$argv[1]" --current-process --tokenize 2>/dev/null)
    while test (count $tokens) -gt 0
        set -l token $tokens[1]
        if string match -qr '^[A-Za-z_][A-Za-z0-9_]*=' -- "$token"
            set -e tokens[1]
            continue
        end
        printf '%s' "$token"
        return 0
    end
    return 1
end

# 首词是不是一个「展开后还是它自己」的普通词。$ ( ) ~ { } % 引号反斜杠这些会把
# 首词换成别的东西,判不出来就交回 fish 自己展开,这里不猜;; & | < > # ^ ! 和空白
# 同理,出现了说明这行有 fish 语法结构。
# 通配符 * ? [ ] 故意不在名单里:命令位上出现通配符,本来就说明这不是个命令名。
# 不能用 \w 白名单——fish 的正则里 \w 只认 ASCII,中文会被当成元字符。
function __yunxi_head_is_plain_word
    test -n "$argv[1]"; or return 1
    string match -qr '[\x27"$()~{}%;&|<>#^!\x5c\s]' -- "$argv[1]"; and return 1
    return 0
end

function __yunxi_hand_to_ai
    set -e __yunxi_image_counter
    __yunxi_wrap_fish_prompt
    set -g __yunxi_cursor_hidden 1
    history append -- "$argv[1]"
    set -g __yunxi_pending_buffer "$argv[1]"
    commandline -b -- ""
    printf '\e[?25l'
    commandline -f execute
end

function __yunxi_accept_line
    status is-interactive; or return

    commandline -f expand-abbr
    set -l buffer (commandline -b | string collect)
    set -l trimmed (string trim -- "$buffer")
    if test -z "$trimmed"
        __yunxi_execute_or_continue
        return
    end

    if not __yunxi_buffer_is_multiline "$buffer"
        # 单行本来靠 fish_command_not_found 兜底,但 fish 是先展开再找命令:
        # 自然语言里带个没匹配上的通配符(「输出这段命令 …/core.*.zst」),
        # fish 在展开阶段就报「未找到通配符的匹配项」,命令根本没开始找,
        # 兜底函数也就永远不触发。首词是普通词又不是任何命令时提前接管。
        set -l head (__yunxi_first_token_raw "$buffer")
        if not __yunxi_head_is_plain_word "$head"; or type -q -- "$head"
            __yunxi_execute_or_continue
            return
        end
        __yunxi_hand_to_ai "$buffer"
        return
    end

    set -l first_command (__yunxi_first_command "$buffer")
    if test -n "$first_command"; and not contains -- "$first_command" time test date which type command history; and type -q -- "$first_command"
        __yunxi_execute_or_continue
        return
    end

    printf '%s' "$buffer" | yunxi --shell-classify --shell fish --stdin 2>/dev/null
    set -l classify_status $status
    if test $classify_status -eq 0
        __yunxi_execute_or_continue
        return
    else if test $classify_status -ne 1
        __yunxi_execute_or_continue
        return
    end

    __yunxi_hand_to_ai "$buffer"
end

bind enter __yunxi_accept_line
bind \r __yunxi_accept_line
bind -M insert enter __yunxi_accept_line
bind -M insert \r __yunxi_accept_line

function fish_command_not_found
    status is-interactive; or return 127

    set -e __yunxi_image_counter

    set -l current_line (status current-commandline 2>/dev/null | string collect)
    if test -n "$current_line"; and not string match -qr '[\n\r]' -- "$current_line"
        set -l top_command (__yunxi_first_command "$current_line")
        if test -z "$top_command"; or not type -q -- "$top_command"
            printf '\n'
            printf '%s' "$current_line" | yunxi --shell-intercept --shell fish --stdin 2>/dev/null
            return 127
        end
    end

    set -l command $argv
    if test (count $command) -eq 0
        return 127
    end

    set -l text (string join ' ' -- $command)
    string match -qr '[\n\r]' -- $text; and return 127

    yunxi --shell-intercept --shell fish -- $command 2>/dev/null
    return 127
end
"#,
    );
    output
}

pub fn install(paths: &YunXiPaths) -> Result<()> {
    if let Some(parent) = paths.fish_hook_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&paths.fish_hook_file, hook())?;
    println!(
        "{}: {}",
        t("installed fish hook", "已安装 fish hook"),
        paths.fish_hook_file.display()
    );
    super::print_reload_hint("fish", &paths.fish_hook_file);
    super::locate::warn_if_unreachable("fish", None);
    Ok(())
}

pub fn uninstall(paths: &YunXiPaths) -> Result<bool> {
    let removed = match std::fs::remove_file(&paths.fish_hook_file) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    if removed {
        println!(
            "{}: fish",
            t("removed YunXi shell hook", "已移除 YunXi shell hook")
        );
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fish_hook_defines_command_not_found_handler() {
        let hook = hook();
        // 视频占位符的标签是 Video:只替 Image 的话第二个视频起序号永远
        // 是 1,解析端会把它们都当成第一个附件(08-28)。
        assert!(hook.contains("string replace -r '^\\[(Image|Video) 1'"));
        assert!(hook.contains("fish_command_not_found"));
        assert!(hook.contains("--shell fish"));
        assert!(hook.contains("status current-commandline 2>/dev/null | string collect"));
        assert!(hook.contains("not type -q -- \"$top_command\"\n            printf '\\n'"));
        assert!(hook.contains("printf '%s' \"$current_line\" | yunxi --shell-intercept"));
        assert!(hook.contains("return 127"));
    }

    /// 单行的自然语言原本全靠 fish_command_not_found 兜底,可 fish 是先展开再找
    /// 命令:句子里带个没匹配上的通配符(「…/core.*.zst」),fish 在展开阶段就报
    /// 「未找到通配符的匹配项」,兜底函数永远不触发。所以回车时先看首词。
    #[test]
    fn fish_hook_routes_single_line_prose_before_fish_expands_globs() {
        let hook = hook();
        // 取未展开的原文:--tokens-expanded 会真的跑命令替换,放在每次回车上按不得。
        // `--current-process --tokenize` 是跨 fish 版本可用的语法分词入口。
        assert!(hook.contains("commandline --input=\"$argv[1]\" --current-process --tokenize"));
        // 真正调用 --tokens-expanded 的只剩原来那一处(多行分支/兜底走它),新路
        // 没再加一处。只数代码行:hook 里的注释也提到这个名字,连注释一起数会
        // 把「改了一句注释」变成红测。
        let expanded_calls = hook
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| line.contains("--tokens-expanded"))
            .count();
        assert_eq!(expanded_calls, 1, "{hook}");
        // 通配符不在首词黑名单里:命令位上出现 * ? [ ],本来就说明这不是命令名。
        let class = "'[\\x27\"$()~{}%;&|<>#^!\\x5c\\s]'";
        assert!(hook.contains(class), "首词黑名单变了:\n{hook}");
        // [ ] 是类的定界符,没法这么查;能查的是这两个。
        for glob in ['*', '?'] {
            assert!(!class.contains(glob), "{glob} 不该进首词黑名单");
        }
        // 单行分支:首词是普通词又不是命令 -> 走 AI,其余交回 fish。
        assert!(hook.contains(
            "if not __yunxi_head_is_plain_word \"$head\"; or type -q -- \"$head\"\n            __yunxi_execute_or_continue"
        ));
        // 单行与多行共用同一条交接路径,别分叉出第二套。
        assert_eq!(hook.matches("__yunxi_hand_to_ai \"").count(), 2);
        assert!(hook.contains("printf '%s' \"$buffer\" | yunxi --shell-intercept"));
    }

    #[test]
    fn fish_hook_defines_curated_top_level_completions() {
        let hook = hook();
        let expected = completion_entries();
        let completion_lines = hook
            .lines()
            .filter(|line| line.starts_with("complete -c yunxi "))
            .collect::<Vec<_>>();

        assert_eq!(completion_lines.len(), expected.len());
        for (command, description) in expected {
            let completion = format!(
                "complete -c yunxi -n __fish_use_subcommand -f -a {command} -d '{description}'"
            );
            assert!(completion_lines.contains(&completion.as_str()));
        }
    }

    #[test]
    fn fish_hook_defines_paste_binding() {
        let hook = hook();
        assert!(hook.contains("__yunxi_paste"));
        assert!(hook.contains("bind \\cv __yunxi_paste"));
        assert!(hook.contains("yunxi --clipboard-paste"));
    }

    #[test]
    fn fish_hook_defines_enter_binding() {
        let hook = hook();
        assert!(hook.contains("__yunxi_accept_line"));
        assert!(hook.contains("__yunxi_wrap_fish_prompt"));
        assert!(hook.contains("functions -c fish_prompt __yunxi_original_fish_prompt"));
        assert!(hook.contains("if set -q __yunxi_pending_buffer"));
        assert!(hook.contains("__yunxi_replay_buffer"));
        assert!(hook.contains("__yunxi_on_prompt --on-event fish_prompt"));
        assert!(hook.contains("__yunxi_replay_buffer \"$buffer\"\n    printf '\\n'"));
        assert!(!hook.contains("        fish_prompt\n"));
        assert!(hook.contains("string length --visible"));
        assert!(hook.contains("printf '\\e[?25l'"));
        assert!(hook.contains("printf '\\e[1A\\e[%sG' $prompt_col"));
        assert!(hook.contains("not set_color $fish_color_error 2>/dev/null"));
        assert!(hook.contains("set_color normal"));
        assert!(hook.contains("printf '\\e[?25h'"));
        assert!(hook.contains("set -g __yunxi_cursor_hidden 1"));
        assert!(hook.contains("set -e __yunxi_cursor_hidden"));
        assert!(hook.contains("return $yunxi_status"));
        assert!(hook.contains("__yunxi_execute_or_continue"));
        assert!(hook.contains("__yunxi_buffer_is_multiline"));
        assert!(hook.contains("test (string split \\n -- \"$argv[1]\" | count) -gt 1"));
        assert!(hook.contains("__yunxi_first_command"));
        assert!(hook.contains("commandline --input=\"$argv[1]\" --tokens-expanded"));
        assert!(hook.contains("commandline --input=\"$argv[1]\" --current-process --tokenize"));
        assert!(hook.contains("type -q -- \"$first_command\""));
        assert!(hook.contains("set -g __yunxi_pending_buffer \"$argv[1]\""));
        assert!(hook.contains("history append -- \"$argv[1]\""));
        assert!(hook.contains("commandline -b -- \"\""));
        assert!(hook.contains("commandline -f execute"));
        assert!(hook.contains("commandline -f expand-abbr"));
        assert!(hook.contains("string match -qr '^[A-Za-z_][A-Za-z0-9_]*='"));
        assert!(!hook.contains("cancel-commandline"));
        assert!(hook.contains("commandline -b | string collect"));
        assert!(!hook.contains("commandline -b | string collect -N"));
        assert!(!hook.contains("__yunxi_multiline_has_unknown_command"));
        assert!(hook.contains("--shell-classify --shell fish --stdin"));
        assert!(hook.contains("--shell-intercept --shell fish --stdin"));
        assert!(hook.contains("bind enter __yunxi_accept_line"));
        assert!(hook.contains("bind \\r __yunxi_accept_line"));
        assert!(hook.contains("bind ctrl-j __yunxi_insert_newline"));
        assert!(hook.contains("bind -M insert enter __yunxi_accept_line"));
        assert!(hook.contains("bind -M insert ctrl-j __yunxi_insert_newline"));
    }

    #[test]
    fn fish_hook_resets_image_counter_on_command_not_found() {
        let hook = hook();
        assert!(hook.contains("set -e __yunxi_image_counter"));
    }

    #[test]
    fn fish_hook_does_not_filter_natural_language_symbols() {
        let hook = hook();
        assert!(!hook.contains("length -- $text) -le 120"));
        assert!(!hook.contains("[/\\"));
        assert!(!hook.contains("=|;&<>"));
    }

    #[test]
    fn uninstall_reports_only_existing_hook() {
        let temp = tempfile::tempdir().unwrap();
        let paths = YunXiPaths {
            root_dir: temp.path().to_path_buf(),
            config_dir: temp.path().to_path_buf(),
            config_file: temp.path().join("config.json"),
            skills_dir: temp.path().join("skills"),
            data_dir: temp.path().join("data"),
            cache_dir: temp.path().join("cache"),
            state_dir: temp.path().join("state"),
            pictures_dir: temp.path().join("pictures"),
            fish_hook_file: temp.path().join("yunxi.fish"),
            bash_hook_file: temp.path().join("bash-hook.sh"),
            zsh_hook_file: temp.path().join("zsh-hook.zsh"),
            scripts_dir: temp.path().join("scripts"),
            system_scripts_dir: PathBuf::new(),
        };

        assert!(!uninstall(&paths).unwrap());
        std::fs::write(&paths.fish_hook_file, hook()).unwrap();
        assert!(uninstall(&paths).unwrap());
        assert!(!uninstall(&paths).unwrap());
    }
}
