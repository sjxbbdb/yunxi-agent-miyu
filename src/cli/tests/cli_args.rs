//! 命令行参数与子命令的解析。

// 被测的东西散在 cli::mod 与 repl 的兄弟模块里，这里全都要够到。
use super::shared::*;
use crate::cli::*;
/// REPL 的 `/models` 收一整串自由文本,`--global` / `-g` 要能从里面摘
/// 出来,并且不能把 `-g` 开头的模型名(如 `-gpt`)误当成开关。
#[test]
fn models_argument_parses_the_global_switch() {
    let plain = parse_models_argument("  gpt-5  ");
    assert!(!plain.global);
    assert_eq!(plain.target.as_deref(), Some("gpt-5"));

    let bare = parse_models_argument("--global");
    assert!(bare.global);
    assert!(bare.target.is_none());

    for input in ["-g gpt-5", "--global gpt-5", "-g --global gpt-5"] {
        let parsed = parse_models_argument(input);
        assert!(parsed.global, "{input}");
        assert_eq!(parsed.target.as_deref(), Some("gpt-5"), "{input}");
    }

    // `-gpt` 是模型名,不是开关。
    let lookalike = parse_models_argument("-gpt-image");
    assert!(!lookalike.global);
    assert_eq!(lookalike.target.as_deref(), Some("-gpt-image"));

    assert!(parse_models_argument("").target.is_none());
}

#[test]
fn variant_is_a_cli_subcommand_with_an_optional_name() {
    let cli = parse_args(["yunxi", "variant"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Variant(VariantArgs { name: None }))
    ));

    let cli = parse_args(["yunxi", "variant", "high"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Variant(VariantArgs { name })) if name.as_deref() == Some("high")
    ));

    assert!(parse_args(
        ["yunxi", "variant", "high", "extra"]
            .map(OsString::from)
            .to_vec()
    )
    .is_err());
}

#[test]
fn continue_and_session_flags_are_mutually_exclusive() {
    let cli = parse_args(["yunxi", "-c", "hello"].map(OsString::from).to_vec()).unwrap();
    assert!(cli.turn.continue_session);
    assert_eq!(cli.message, vec!["hello".to_string()]);

    let cli = parse_args(
        ["yunxi", "--session", "2", "hello"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(!cli.turn.continue_session);
    assert_eq!(cli.turn.session.as_deref(), Some("2"));

    assert!(parse_args(
        ["yunxi", "-c", "--session", "2", "hello"]
            .map(OsString::from)
            .to_vec()
    )
    .is_err());
}

#[test]
fn picker_keys_reach_delete_only_through_a_modifier() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let plain = KeyModifiers::NONE;
    let control = KeyModifiers::CONTROL;

    // Every printable character is search input, so a bare `d` must never
    // be a shortcut — deletion needs Ctrl+D (or the Delete key).
    assert_eq!(
        inline_select_key(KeyCode::Char('d'), plain, true),
        InlineSelectKey::Char('d')
    );
    assert_eq!(
        inline_select_key(KeyCode::Char('d'), control, true),
        InlineSelectKey::DeleteRequest
    );
    assert_eq!(
        inline_select_key(KeyCode::Delete, plain, true),
        InlineSelectKey::DeleteRequest
    );

    // Pickers that did not opt in stay exactly as they were.
    assert_eq!(
        inline_select_key(KeyCode::Char('d'), control, false),
        InlineSelectKey::Ignore
    );
    assert_eq!(
        inline_select_key(KeyCode::Delete, plain, false),
        InlineSelectKey::Ignore
    );

    assert_eq!(
        inline_select_key(KeyCode::Char('c'), control, true),
        InlineSelectKey::Cancel
    );
    assert_eq!(
        inline_select_key(KeyCode::Esc, plain, true),
        InlineSelectKey::Cancel
    );
    assert_eq!(
        inline_select_key(KeyCode::Enter, plain, true),
        InlineSelectKey::Accept
    );
    assert_eq!(
        inline_select_key(KeyCode::Char('j'), plain, true),
        InlineSelectKey::Down
    );
    assert_eq!(
        inline_select_key(KeyCode::Char('k'), plain, true),
        InlineSelectKey::Up
    );
}

#[test]
fn web_is_a_cli_subcommand_with_local_server_options() {
    let cli = parse_args(
        ["yunxi", "web", "--port", "4100"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Web(WebArgs {
            port: 4100,
            bind: None,
            port_explicit: true,
        }))
    ));

    for arg in ["stop", "status", "restart", "--status", "--stop"] {
        assert!(parse_args(["yunxi", "web", arg].map(OsString::from).to_vec()).is_err());
    }

    let cli = parse_args(["yunxi", "web"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Web(WebArgs {
            port: 8300,
            bind: None,
            port_explicit: false,
        }))
    ));

    for args in [
        vec!["yunxi", "web", "-p"],
        vec!["yunxi", "web", "--password-file", "/tmp/x"],
    ] {
        assert!(parse_args(args.into_iter().map(OsString::from).collect()).is_err());
    }

    assert!(parse_args(["yunxi", "web", "--public"].map(OsString::from).to_vec(),).is_err());
}

#[test]
fn bare_web_does_not_override_the_persisted_launch_config() {
    let temp = tempfile::tempdir().unwrap();
    let paths = pop_test_paths(temp.path());
    let args = WebArgs {
        port: ipc::DEFAULT_WEB_PORT,
        bind: None,
        port_explicit: false,
    };

    assert!(web_launch_config(&paths, &args).unwrap().is_none());
}

#[test]
fn daemon_owns_lifecycle_and_log_commands() {
    for (arg, expected) in [
        ("start", "start"),
        ("stop", "stop"),
        ("restart", "restart"),
        ("status", "status"),
    ] {
        let cli = parse_args(["yunxi", "daemon", arg].map(OsString::from).to_vec()).unwrap();
        let actual = match cli.command {
            Some(Command::Daemon(DaemonArgs {
                command: Some(DaemonCommand::Start),
                ..
            })) => "start",
            Some(Command::Daemon(DaemonArgs {
                command: Some(DaemonCommand::Stop),
                ..
            })) => "stop",
            Some(Command::Daemon(DaemonArgs {
                command: Some(DaemonCommand::Restart),
                ..
            })) => "restart",
            Some(Command::Daemon(DaemonArgs {
                command: Some(DaemonCommand::Status),
                ..
            })) => "status",
            other => panic!("unexpected command: {other:?}"),
        };
        assert_eq!(actual, expected);
    }

    let cli = parse_args(["yunxi", "daemon", "logs"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Daemon(DaemonArgs {
            command: Some(DaemonCommand::Logs(DaemonLogsArgs { lines: None, .. })),
            ..
        }))
    ));

    let cli = parse_args(
        ["yunxi", "daemon", "logs", "-n", "25"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Daemon(DaemonArgs {
            command: Some(DaemonCommand::Logs(DaemonLogsArgs {
                lines: Some(25),
                ..
            })),
            ..
        }))
    ));
}

#[test]
fn reload_is_a_top_level_command() {
    let cli = parse_args(["yunxi", "reload"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(cli.command, Some(Command::Reload)));
    assert!(parse_args(["yunxi", "reload", "extra"].map(OsString::from).to_vec()).is_err());
}

#[test]
fn daemon_accepts_a_port_and_defaults_to_start() {
    let cli = parse_args(
        ["yunxi", "daemon", "--port", "9412"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Daemon(DaemonArgs {
            port: Some(9412),
            command: None,
        }))
    ));

    let cli = parse_args(
        ["yunxi", "daemon", "--port", "9412", "restart"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Daemon(DaemonArgs {
            port: Some(9412),
            command: Some(DaemonCommand::Restart),
        }))
    ));

    let cli = parse_args(
        ["yunxi", "daemon", "start", "--port", "9412"]
            .map(OsString::from)
            .to_vec(),
    )
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Daemon(DaemonArgs {
            port: Some(9412),
            command: Some(DaemonCommand::Start),
        }))
    ));

    assert!(parse_args(
        ["yunxi", "daemon", "--password"]
            .map(OsString::from)
            .to_vec(),
    )
    .is_err());
}

#[test]
fn daemon_web_urls_are_rendered_on_separate_aligned_lines() {
    let urls = vec![
        "http://127.0.0.1:8300".to_string(),
        "http://192.168.1.2:8300".to_string(),
    ];
    assert_eq!(
        daemon_web_status_lines("WebUI:", &urls),
        [
            "WebUI: http://127.0.0.1:8300",
            "       http://192.168.1.2:8300",
        ]
    );
    assert_eq!(
        daemon_web_status_lines("WebUI：", &urls),
        [
            "WebUI： http://127.0.0.1:8300",
            "        http://192.168.1.2:8300",
        ]
    );
}

#[test]
fn pop_is_a_cli_subcommand_with_an_optional_count() {
    let cli = parse_args(["yunxi", "pop"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Pop(PopArgs { count: None, .. }))
    ));

    let cli = parse_args(["yunxi", "pop", "3"].map(OsString::from).to_vec()).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Pop(PopArgs { count: Some(3), .. }))
    ));
    assert!(parse_args(["yunxi", "pop", "0"].map(OsString::from).to_vec()).is_err());
    assert!(parse_args(["yunxi", "pop", "nope"].map(OsString::from).to_vec()).is_err());
}

#[test]
fn debug_is_a_global_cli_option() {
    for args in [
        &["yunxi", "--debug", "models", "1"][..],
        &["yunxi", "models", "--debug", "1"][..],
        &["yunxi", "hello", "--debug"][..],
        &["yunxi", "ask", "hello", "--debug"][..],
    ] {
        let cli = parse_args(args.iter().map(OsString::from).collect()).unwrap();
        assert!(cli.debug);
    }

    let cli = parse_args(["yunxi", "hello", "--debug"].map(OsString::from).to_vec()).unwrap();
    assert_eq!(cli.message, ["hello"]);

    let cli = parse_args(["yunxi", "--", "--debug"].map(OsString::from).to_vec()).unwrap();
    assert!(!cli.debug);
    assert_eq!(cli.message, ["--debug"]);
}

/// `yunxi session sandbox <会话> <目录> --allow-read`:开关解析得出来,且与
/// `--clear` 互斥、离开目录就不成立(clap 的 requires/conflicts 接的是字段名,
/// 写错在 release 档是静默失效,所以这里钉一遍)。
#[test]
fn session_sandbox_parses_the_allow_read_switch() {
    let parse = |args: &[&str]| {
        parse_args(args.iter().map(OsString::from).collect()).map(|cli| match cli.command {
            Some(Command::Session(args)) => args.command,
            other => panic!("expected a session subcommand, got {other:?}"),
        })
    };
    let command = parse(&[
        "yunxi",
        "session",
        "sandbox",
        "work",
        "/tmp/proj",
        "--allow-read",
    ])
    .expect("valid invocation");
    match command {
        SessionCommand::Sandbox {
            target,
            dir,
            clear,
            allow_read,
        } => {
            assert_eq!(target, "work");
            assert_eq!(dir.as_deref(), Some(std::path::Path::new("/tmp/proj")));
            assert!(!clear);
            assert!(allow_read);
        }
        other => panic!("expected sandbox, got {other:?}"),
    }
    let plain = parse(&["yunxi", "session", "sandbox", "work", "/tmp/proj"]).expect("valid");
    assert!(matches!(
        plain,
        SessionCommand::Sandbox {
            allow_read: false,
            ..
        }
    ));
    assert!(parse(&[
        "yunxi",
        "session",
        "sandbox",
        "work",
        "--clear",
        "--allow-read"
    ])
    .is_err());
    assert!(parse(&["yunxi", "session", "sandbox", "work", "--allow-read"]).is_err());
}

#[test]
fn session_selection_defaults_to_the_current_entry() {
    let entry = |id: &str, is_current: bool| SessionListEntry {
        context_tokens: None,
        id: id.to_string(),
        name: id.to_string(),
        is_current,
        turns: 0,
        snippet: String::new(),
        sandbox: None,
        sandbox_read_all: false,
        mode: "normal".to_string(),
        running: false,
    };
    let entries = vec![entry("default", true), entry("active", false)];

    assert_eq!(session_initial_selection(&entries, Some("active")), 1);
    assert_eq!(session_initial_selection(&entries, None), 0);
    assert!(matches!(
        session_ref_from_index(&entries, 2),
        Some(yunxi_core::ipc::SessionRef::Id { id }) if id == "active"
    ));
    assert_eq!(session_initial_selection(&[entry("only", false)], None), 0);
}

/// `pm` 暂不公开:帮助与补全里看不到它,但显式 `yunxi pm …` 与 `yunxipm …` 一字未改。
/// 不能靠删解析分支来隐藏——根命令吃 trailing_var_arg,删了 `yunxi pm list` 会被
/// 当成一句聊天发出去。
#[test]
fn pm_is_hidden_from_help_but_still_dispatches() {
    let visible: Vec<String> = Cli::command()
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| sub.get_name().to_string())
        .collect();
    assert!(
        !visible.iter().any(|name| name == "pm"),
        "公开命令表里还有 pm: {visible:?}"
    );

    let help = localized_command().render_long_help().to_string();
    assert!(!help.contains("\n  pm "), "根帮助里还列着 pm:\n{help}");

    let cli = parse_args(["yunxi", "pm", "list"].map(OsString::from).to_vec()).unwrap();
    assert!(
        matches!(cli.command, Some(Command::Pm(_))),
        "显式 yunxi pm 应照旧进包管理"
    );

    let shimmed = apply_pm_shim(["/usr/bin/yunxipm", "list"].map(OsString::from).to_vec());
    let cli = parse_args(shimmed).unwrap();
    assert!(
        matches!(cli.command, Some(Command::Pm(_))),
        "yunxipm shim 应照旧进包管理"
    );
}
