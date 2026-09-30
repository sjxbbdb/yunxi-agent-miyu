//! 工具轮之间只有空白的「正文」（09-27 真机：DeepSeek 在思考和工具调用之间吐了一段 `"\n\n"`）。
//!
//! 它不是她开口说话：不该把这一段过程收成一行，也不该画出空行。以前收段留一行空、
//! 两个换行原样画成两行空、接工具时再补一行收尾，收缩行下面空出一大块。

use crate::cli::repl::tail::screen::term::Term;
use crate::cli::tests::tui_blocks::with_blocks;
use yunxi_core::llm::{ChatStreamChunk, ChatStreamKind};
use yunxi_hosts::render::{
    set_cols_override, ReasoningDisplayMode, StreamRenderer, ToolCallDisplayMode,
};

fn renderer() -> StreamRenderer {
    let mut renderer = StreamRenderer::new(
        ReasoningDisplayMode::Summary,
        ToolCallDisplayMode::Summary,
        false,
        true,
        4,
    );
    renderer.use_external_cursor_control();
    renderer.use_buffered_output();
    renderer.use_terminal_surface();
    renderer
}

fn lines(term: &Term) -> Vec<String> {
    (0..term.line_count())
        .map(|i| {
            term.row_spans(i)
                .into_iter()
                .map(|span| span.text)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn chunk(r: &mut StreamRenderer, t: &mut Term, kind: ChatStreamKind, text: &str) {
    r.write_chunk(ChatStreamChunk {
        kind,
        text: text.to_string(),
    })
    .unwrap();
    t.feed(&r.take_output_frame());
}

fn tool(r: &mut StreamRenderer, t: &mut Term, cmd: &str) {
    r.write_tool_call("run_command", &format!(r#"{{"command":"{cmd}"}}"#))
        .unwrap();
    t.feed(&r.take_output_frame());
    r.write_tool_result("run_command", true, "done").unwrap();
    t.feed(&r.take_output_frame());
    r.tick_spinner().unwrap();
    t.feed(&r.take_output_frame());
}

fn round(r: &mut StreamRenderer, t: &mut Term, cmd: &str) {
    chunk(
        r,
        t,
        ChatStreamKind::Reasoning,
        "Let me think about this.\n",
    );
    tool(r, t, cmd);
}

fn has_blank_run(rows: &[String]) -> bool {
    rows.windows(2)
        .any(|pair| pair[0].is_empty() && pair[1].is_empty())
}

#[test]
fn whitespace_only_content_between_tool_rounds_neither_folds_nor_leaves_blank_rows() {
    set_cols_override(100);
    with_blocks(|| {
        let mut r = renderer();
        let mut t = Term::default();
        t.set_cols(100);
        round(&mut r, &mut t, "ls a");
        chunk(&mut r, &mut t, ChatStreamKind::Reasoning, "hmm.\n");
        chunk(&mut r, &mut t, ChatStreamKind::Content, "\n\n");
        tool(&mut r, &mut t, "ls b");
        chunk(&mut r, &mut t, ChatStreamKind::Content, " \n");
        round(&mut r, &mut t, "ls c");
        let rows = lines(&t);
        let text = rows.join("\n");
        assert!(!text.contains('›'), "空白正文不该把过程收成一行：\n{text}");
        assert!(!has_blank_run(&rows), "不该有连着的空行：\n{text}");
    });
    set_cols_override(0);
}

fn speak(pieces: &[&str]) -> Vec<String> {
    let mut r = renderer();
    let mut t = Term::default();
    t.set_cols(100);
    round(&mut r, &mut t, "ls a");
    for piece in pieces {
        chunk(&mut r, &mut t, ChatStreamKind::Content, piece);
    }
    r.finish().unwrap();
    t.feed(&r.take_output_frame());
    lines(&t)
}

/// 真正开口时，开头那几行空吞掉，排出来和开头没有空行的正文一模一样。
#[test]
fn leading_blank_lines_of_real_content_are_dropped() {
    set_cols_override(100);
    with_blocks(|| {
        let plain = speak(&["好了，查到了。\n\n第二段。\n"]);
        let padded = speak(&["\n\n", "\n好了，查到了。\n\n第二段。\n"]);
        assert_eq!(padded, plain, "开头的空行不该改变排版");
        assert!(plain.join("\n").contains('›'), "开口说话时过程照常收成一行");
    });
    set_cols_override(0);
}
