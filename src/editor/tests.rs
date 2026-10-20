//! Headless editor tests: feed vim keys, check buffer text and cursor.

use super::*;
use crate::buffer::{Buffer, FileType};
use crate::key::parse_keys;

fn ed(text: &str) -> Editor {
    let mut e = Editor::new(80, 24);
    let id = e.win().buf;
    *e.buffers.get_mut(&id).unwrap() = Buffer::from_str(id, text);
    e.buf_mut().filetype = FileType::Text;
    e
}

fn md(text: &str) -> Editor {
    let mut e = ed(text);
    e.buf_mut().filetype = FileType::Markdown;
    e
}

fn feed(e: &mut Editor, keys: &str) {
    for k in parse_keys(keys, " ") {
        e.handle_key(k);
    }
}

fn run(text: &str, keys: &str) -> String {
    let mut e = ed(text);
    feed(&mut e, keys);
    e.buf().contents()
}

fn cur(e: &Editor) -> (usize, usize) {
    (e.cursor().line, e.cursor().col)
}

#[test]
fn word_motions() {
    let mut e = ed("foo bar.baz  qux\n\nnext line");
    feed(&mut e, "w");
    assert_eq!(cur(&e), (0, 4));
    feed(&mut e, "w");
    assert_eq!(cur(&e), (0, 7));
    feed(&mut e, "W");
    assert_eq!(cur(&e), (0, 13));
    feed(&mut e, "w");
    assert_eq!(cur(&e), (1, 0), "empty line is a word");
    feed(&mut e, "w");
    assert_eq!(cur(&e), (2, 0));
    feed(&mut e, "b");
    assert_eq!(cur(&e), (1, 0));
    feed(&mut e, "b");
    assert_eq!(cur(&e), (0, 13));
    feed(&mut e, "0e");
    assert_eq!(cur(&e), (0, 2));
    feed(&mut e, "E");
    assert_eq!(cur(&e), (0, 10));
    feed(&mut e, "ge");
    assert_eq!(cur(&e), (0, 7));
    feed(&mut e, "3b");
    assert_eq!(cur(&e), (0, 0));
}

#[test]
fn line_motions_and_find() {
    let mut e = ed("  hello world");
    feed(&mut e, "$");
    assert_eq!(cur(&e), (0, 12));
    feed(&mut e, "^");
    assert_eq!(cur(&e), (0, 2));
    feed(&mut e, "0");
    assert_eq!(cur(&e), (0, 0));
    feed(&mut e, "fo");
    assert_eq!(cur(&e), (0, 6));
    feed(&mut e, ";");
    assert_eq!(cur(&e), (0, 9));
    feed(&mut e, ",");
    assert_eq!(cur(&e), (0, 6));
    feed(&mut e, "tl");
    assert_eq!(cur(&e), (0, 10));
    feed(&mut e, "Fh");
    assert_eq!(cur(&e), (0, 2));
}

#[test]
fn vertical_motions_keep_column() {
    let mut e = ed("abcdef\nab\nabcdef");
    feed(&mut e, "4lj");
    assert_eq!(cur(&e), (1, 1));
    feed(&mut e, "j");
    assert_eq!(cur(&e), (2, 4));
    feed(&mut e, "gg$G");
    assert_eq!(cur(&e), (2, 5));
    feed(&mut e, "2G");
    assert_eq!(cur(&e), (1, 1), "nostartofline keeps the column");
}

#[test]
fn match_pair_and_paragraphs() {
    let mut e = ed("f(a, [b]) {\n}\n\npara");
    feed(&mut e, "%");
    assert_eq!(cur(&e), (0, 8));
    feed(&mut e, "%");
    assert_eq!(cur(&e), (0, 1));
    feed(&mut e, "$%");
    assert_eq!(cur(&e), (1, 0));
    feed(&mut e, "gg}");
    assert_eq!(cur(&e), (2, 0));
    feed(&mut e, "}");
    assert_eq!(cur(&e), (3, 3));
    feed(&mut e, "{");
    assert_eq!(cur(&e), (2, 0));
}

#[test]
fn delete_operators() {
    assert_eq!(run("one two three", "dw"), "two three");
    assert_eq!(run("one two three", "2dw"), "three");
    assert_eq!(run("one two three", "wd$"), "one ");
    assert_eq!(run("one two three", "wD"), "one ");
    assert_eq!(run("one two\nthree", "wdw"), "one \nthree");
    assert_eq!(run("a\nb\nc\nd", "jdd"), "a\nc\nd");
    assert_eq!(run("a\nb\nc\nd", "2dd"), "c\nd");
    assert_eq!(run("a\nb\nc\nd", "Gdd"), "a\nb\nc");
    assert_eq!(run("a\nb\nc\nd", "dj"), "c\nd");
    assert_eq!(run("a\nb\nc\nd", "Gdk"), "a\nb");
    assert_eq!(run("abc", "x"), "bc");
    assert_eq!(run("abc", "$X"), "ac");
    assert_eq!(run("abc", "3x"), "");
    assert_eq!(run("abc def", "dfd"), "ef");
    assert_eq!(run("abc def", "dtd"), "def");
    assert_eq!(run("a\nb\n\nc", "d}"), "\nc");
    assert_eq!(run("a b\nc\nd", "wd}"), "a ");
    assert_eq!(run("x\ny\nz", "dG"), "");
    assert_eq!(run("x\ny\nz", "jdgg"), "z");
}

#[test]
fn change_operators() {
    let mut e = ed("one two three");
    feed(&mut e, "cwONE<Esc>");
    assert_eq!(e.buf().contents(), "ONE two three");
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(cur(&e), (0, 2));
    assert_eq!(run("one two", "wcwX<Esc>"), "one X");
    assert_eq!(run("a b", "lcwX<Esc>"), "aXb");
    assert_eq!(run("one two", "Cz<Esc>"), "z");
    assert_eq!(run("one two", "ccnew<Esc>"), "new");
    assert_eq!(run("  indented", "Snew<Esc>"), "  new");
    assert_eq!(run("abc", "sX<Esc>"), "Xbc");
    assert_eq!(run("a\nb\nc", "jcjX<Esc>"), "a\nX");
}

#[test]
fn text_objects() {
    assert_eq!(run("foo bar baz", "wdiw"), "foo  baz");
    assert_eq!(run("foo bar baz", "wdaw"), "foo baz");
    assert_eq!(run("foo bar", "wdaw"), "foo");
    assert_eq!(run("f(a, b)", "fadi("), "f()");
    assert_eq!(run("f(a, (b))", "fbda("), "f(a, )");
    assert_eq!(run("f(a, (b))", "fb2di("), "f()");
    assert_eq!(run("x = \"hello world\";", "fhdi\""), "x = \"\";");
    assert_eq!(run("x = \"hi\" + 1", "fhda\""), "x = + 1");
    assert_eq!(run("say 'a' now", "di'"), "say '' now");
    assert_eq!(run("a [b c] d", "fbci[X<Esc>"), "a [X] d");
    assert_eq!(run("<p>text</p>", "fedit"), "<p></p>");
    assert_eq!(run("<a><b>x</b></a>", "fxdat"), "<a></a>");
    assert_eq!(run("p1\np1\n\np2", "dap"), "p2");
    assert_eq!(run("p1\np1\n\np2", "dip"), "\np2");
    assert_eq!(run("fn() {\n    body\n}", "jdi{"), "fn() {\n}");
    assert_eq!(run("a. Second one. Third.", "fSdis"), "a.  Third.");
    assert_eq!(run("a. Second one. Third.", "fSdas"), "a. Third.");
}

#[test]
fn yank_put() {
    assert_eq!(run("a\nb", "yyp"), "a\na\nb");
    assert_eq!(run("a\nb", "jyyP"), "a\nb\nb");
    assert_eq!(run("a\nb", "yy3p"), "a\na\na\na\nb");
    assert_eq!(run("hello world", "yiwwP"), "hello helloworld");
    assert_eq!(run("hello world", "yiw$p"), "hello worldhello");
    assert_eq!(run("ab", "xp"), "ba");
    assert_eq!(run("a\nb", "ddp"), "b\na");
    let mut e = ed("one two");
    feed(&mut e, "yw");
    assert_eq!(e.registers.get('0').unwrap().text, "one ");
    feed(&mut e, "\"ayiw\"Ayiw");
    assert_eq!(e.registers.get('a').unwrap().text, "oneone");
    feed(&mut e, "\"_dd");
    assert_eq!(e.registers.get('"').unwrap().text, "oneone");
}

#[test]
fn registers_numbered() {
    let mut e = ed("a\nb\nc");
    feed(&mut e, "dddd");
    assert_eq!(e.registers.get('1').unwrap().text, "b");
    assert_eq!(e.registers.get('2').unwrap().text, "a");
    feed(&mut e, "\"2p");
    assert_eq!(e.buf().contents(), "c\na");
}

#[test]
fn indent_join_case_replace() {
    assert_eq!(run("a\nb", ">>"), "    a\nb");
    assert_eq!(run("a\nb", ">j"), "    a\n    b");
    assert_eq!(run("        a", "<<"), "    a");
    assert_eq!(run("a\n   b\nc", "J"), "a b\nc");
    assert_eq!(run("a\nb\nc", "3J"), "a b c");
    assert_eq!(run("a\n b", "gJ"), "a b");
    assert_eq!(run("abc", "~~"), "ABc");
    assert_eq!(run("hello world", "gUiw"), "HELLO world");
    assert_eq!(run("HELLO", "guu"), "hello");
    assert_eq!(run("Hello", "g~~"), "hELLO");
    assert_eq!(run("abc", "rx"), "xbc");
    assert_eq!(run("abc", "2rx"), "xxc");
    assert_eq!(run("abcd", "lRXY<Esc>"), "aXYd");
    assert_eq!(run("abc", "RXYZW<BS><BS><Esc>"), "XYc");
    assert_eq!(run("x 9 y", "<C-a>"), "x 10 y");
    assert_eq!(run("x 9 y", "5<C-x>"), "x 4 y");
    assert_eq!(run("x -1", "<C-a>"), "x 0");
    assert_eq!(run("0x0f", "<C-a>"), "0x10");
}

#[test]
fn undo_redo() {
    let mut e = ed("hello");
    feed(&mut e, "ihey <Esc>");
    feed(&mut e, "A!!<Esc>");
    assert_eq!(e.buf().contents(), "hey hello!!");
    feed(&mut e, "u");
    assert_eq!(e.buf().contents(), "hey hello");
    feed(&mut e, "u");
    assert_eq!(e.buf().contents(), "hello");
    assert!(!e.buf().is_modified());
    feed(&mut e, "<C-r>");
    assert_eq!(e.buf().contents(), "hey hello");
    feed(&mut e, "dd");
    assert_eq!(e.buf().contents(), "");
    feed(&mut e, "u");
    assert_eq!(e.buf().contents(), "hey hello");
}

#[test]
fn dot_repeat() {
    assert_eq!(run("a b c d", "dw.."), "d");
    assert_eq!(run("a\nb\nc", "A;<Esc>j.j."), "a;\nb;\nc;");
    assert_eq!(run("foo foo foo", "cwbar<Esc>w.w."), "bar bar bar");
    assert_eq!(run("a b c d e", "dw3."), "e");
    assert_eq!(run("x\ny", "ox<Esc>."), "x\nx\nx\ny");
    assert_eq!(run("a\nb\nc\nd", "Vjd."), "");
    assert_eq!(run("abc", "~."), "ABc");
}

#[test]
fn counted_insert() {
    assert_eq!(run("", "3ihi <Esc>"), "hi hi hi ");
    assert_eq!(run("x", "2ox<Esc>"), "x\nx\nx");
    assert_eq!(run("a", "3A-<Esc>"), "a---");
}

#[test]
fn insert_keys() {
    assert_eq!(run("", "ifoo bar<C-w>baz<Esc>"), "foo baz");
    assert_eq!(run("", "i  foo<C-u>x<Esc>"), "  x");
    assert_eq!(run("ab", "ix<BS><BS><Esc>"), "ab");
    assert_eq!(run("a\nb", "ji<BS><Esc>"), "ab");
    assert_eq!(run("", "ia<C-t><Esc>"), "    a");
    assert_eq!(run("hello", "yiwo<C-r>\"<Esc>"), "hello\nhello");
    assert_eq!(run("hello\n", "o<C-y><C-y><Esc>"), "hello\nhe");
    assert_eq!(run("", "ifoobar foo<C-p><Esc>"), "foobar foobar");
    assert_eq!(run("abc", "A<C-o>0X<Esc>"), "Xabc");
}

#[test]
fn autoindent() {
    assert_eq!(run("    a", "ob<Esc>"), "    a\n    b");
    assert_eq!(run("    a", "A<CR>b<Esc>"), "    a\n    b");
    assert_eq!(run("    a", "Ob<Esc>"), "    b\n    a");
}

#[test]
fn visual_mode() {
    assert_eq!(run("abcdef", "lvld"), "adef");
    assert_eq!(run("a\nb\nc", "Vjd"), "c");
    assert_eq!(run("ab\ncd\nef", "l<C-v>jjd"), "a\nc\ne");
    assert_eq!(run("ab\ncd", "<C-v>jIX<Esc>"), "Xab\nXcd");
    assert_eq!(run("ab\ncdef", "<C-v>j$AX<Esc>"), "abX\ncdefX");
    assert_eq!(run("abc\nd", "<C-v>jlAX<Esc>"), "aXbc\ndX");
    assert_eq!(run("abc\nd", "<C-v>ljAX<Esc>"), "abXc\nd X");
    assert_eq!(run("hello world", "viwy$p"), "hello worldhello");
    assert_eq!(run("hello world", "wviwU"), "hello WORLD");
    assert_eq!(run("a\nb", "Vj>"), "    a\n    b");
    assert_eq!(run("abc", "vlrx"), "xxc");
    assert_eq!(run("a\nb\nc", "VjJ"), "a b\nc");
    assert_eq!(run("one two", "yiwwviwp"), "one one");
    assert_eq!(run("abc", "lvc<Esc>"), "ac");
    assert_eq!(run("a\nb", "vjo<Esc>x"), "\nb");
    let mut e = ed("abc def");
    feed(&mut e, "vee");
    assert_eq!(cur(&e), (0, 6));
    feed(&mut e, "<Esc>gv");
    assert!(e.mode.is_visual());
    feed(&mut e, "d");
    assert_eq!(e.buf().contents(), "");
}

#[test]
fn visual_text_object_and_counts() {
    assert_eq!(run("f(a b)", "fava(d"), "f");
    assert_eq!(run("p1\np1\n\np2", "vipd"), "\np2");
    assert_eq!(run("1\n2\n3", "VG<C-a>"), "2\n3\n4");
    assert_eq!(run("0\n0\n0", "VGg<C-a>"), "1\n2\n3");
}

#[test]
fn macros() {
    assert_eq!(run("a\nb\nc", "qqA!<Esc>jq2@q"), "a!\nb!\nc!");
    assert_eq!(run("1\n2\n3", "qa0<C-a>jq@a@@"), "2\n3\n4");
    let mut e = ed("x");
    feed(&mut e, "qwiab<Esc>q");
    assert_eq!(e.registers.get('w').unwrap().text, "iab\x1b");
}

#[test]
fn marks_and_jumps() {
    let mut e = ed("a\nb\nc\nd");
    feed(&mut e, "jmaGd'a");
    assert_eq!(e.buf().contents(), "a");
    let mut e = ed("a\nb\nc\nd");
    feed(&mut e, "G");
    assert_eq!(cur(&e), (3, 0));
    feed(&mut e, "<C-o>");
    assert_eq!(cur(&e), (0, 0));
    feed(&mut e, "<Tab>");
    assert_eq!(cur(&e), (3, 0));
    feed(&mut e, "''");
    assert_eq!(cur(&e), (0, 0));
}

#[test]
fn search() {
    let mut e = ed("foo bar\nbaz foo\nfoo");
    feed(&mut e, "/foo<CR>");
    assert_eq!(cur(&e), (1, 4));
    feed(&mut e, "n");
    assert_eq!(cur(&e), (2, 0));
    feed(&mut e, "n");
    assert_eq!(cur(&e), (0, 0), "wraps");
    feed(&mut e, "N");
    assert_eq!(cur(&e), (2, 0));
    feed(&mut e, "?bar<CR>");
    assert_eq!(cur(&e), (0, 4));
    feed(&mut e, "gg0*");
    assert_eq!(cur(&e), (1, 4));
    feed(&mut e, "#");
    assert_eq!(cur(&e), (0, 0));
    assert_eq!(run("a b c x d", "d/x<CR>"), "x d");
    assert_eq!(run("Foo foo", "/foo<CR>x"), "Foo oo");
    assert_eq!(run("Foo foo", "w/Foo<CR>x"), "oo foo");
}

#[test]
fn ex_substitute() {
    assert_eq!(run("a a\na", ":s/a/b/<CR>"), "b a\na");
    assert_eq!(run("a a\na", ":%s/a/b/g<CR>"), "b b\nb");
    assert_eq!(run("hello world", ":s/\\(\\w+\\) \\(\\w+\\)/x/<CR>"), "hello world");
    assert_eq!(run("hello world", r":s/(\w+) (\w+)/\2 \1/<CR>"), "world hello");
    assert_eq!(run("hello", r":s/h\w+/\u&/<CR>"), "Hello");
    assert_eq!(run("hello", r":s/.*/\U&/<CR>"), "HELLO");
    assert_eq!(run("a,b", r":s/,/\r/<CR>"), "a\nb");
    assert_eq!(run("x/y", r":s#/#-#<CR>"), "x-y");
    assert_eq!(run("a\nb\nc", ":2,3s/^/> /<CR>"), "a\n> b\n> c");
    assert_eq!(run("a\na", ":s/a/b/<CR>j:&<CR>"), "b\nb");
    assert_eq!(run("one\ntwo\nthree", "Vj:s/o/0/g<CR>"), "0ne\ntw0\nthree");
    let mut e = ed("abc");
    feed(&mut e, ":s/z/y/<CR>");
    assert_eq!(e.messages[0].0, "E486: Pattern not found: z");
}

#[test]
fn ex_global_and_lines() {
    assert_eq!(run("a1\nb\na2\nc", ":g/a/d<CR>"), "b\nc");
    assert_eq!(run("a1\nb\na2\nc", ":v/a/d<CR>"), "a1\na2");
    assert_eq!(run("a\nb\nc", ":g/./m0<CR>"), "c\nb\na");
    assert_eq!(run("a\nb", ":g/./norm A;<CR>"), "a;\nb;");
    assert_eq!(run("a\nb\nc\nd", ":2,3d<CR>"), "a\nd");
    assert_eq!(run("a\nb\nc", ":3m0<CR>"), "c\na\nb");
    assert_eq!(run("a\nb\nc", ":1m$<CR>"), "b\nc\na");
    assert_eq!(run("a\nb", ":1t.<CR>"), "a\na\nb");
    assert_eq!(run("a\nb", ":%t$<CR>"), "a\nb\na\nb");
    assert_eq!(run("c\na\nb", ":sort<CR>"), "a\nb\nc");
    assert_eq!(run("10\n9\n100", ":sort n<CR>"), "9\n10\n100");
    assert_eq!(run("a\nb\na", ":sort u<CR>"), "a\nb");
    assert_eq!(run("a\nb\nc", ":sort!<CR>"), "c\nb\na");
    assert_eq!(run("a\nb\nc", ":%j<CR>"), "a b c");
    assert_eq!(run("a\nb", ":%><CR>"), "    a\n    b");
    assert_eq!(run("a\nb\nc", ":2<CR>dd"), "a\nc");
    assert_eq!(run("a\nb\nc", ":$<CR>dd"), "a\nb");
    assert_eq!(run("a\nb\nc", ":/c/d<CR>"), "a\nb");
    assert_eq!(run("a\nb\nc", ":.,+1d<CR>"), "c");
    assert_eq!(run("a\nb", ":%norm Ax<CR>"), "ax\nbx");
    assert_eq!(run("a\nb", "yy:pu<CR>"), "a\na\nb");
    assert_eq!(run("a\nb", ":%d<CR>u"), "a\nb");
    assert_eq!(run("a1\nb\na2", ":g/a/s/$/!/<CR>u"), "a1\nb\na2");
}

#[test]
fn ex_set_and_maps() {
    let mut e = ed("a");
    feed(&mut e, ":set sw=2 nonumber number ts+=4<CR>");
    assert_eq!(e.options.shiftwidth, 2);
    assert!(e.options.number);
    assert_eq!(e.options.tabstop, 8);
    feed(&mut e, ":set invnumber<CR>");
    assert!(!e.options.number);
    feed(&mut e, ">>");
    assert_eq!(e.buf().contents(), "  a");
    feed(&mut e, ":set foo<CR>");
    assert!(e.messages[0].0.contains("E518"));
    let mut e = ed("abc");
    feed(&mut e, ":inoremap jk <lt>Esc><CR>");
    feed(&mut e, "Axjk");
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(e.buf().contents(), "abcx");
    // A pending prefix that does not complete is inserted as text.
    feed(&mut e, "Ajz<Esc>");
    assert_eq!(e.buf().contents(), "abcxjz");
    feed(&mut e, ":nnoremap Q dd<CR>Q");
    assert_eq!(e.buf().contents(), "");
    let mut e = ed("x");
    feed(&mut e, ":nmap <lt>leader>d dd<CR> d");
    assert_eq!(e.buf().contents(), "");
}

#[test]
fn map_timeout_resolves() {
    let mut e = ed("abc");
    feed(&mut e, ":inoremap jk <lt>Esc><CR>Aj");
    assert!(e.has_pending_map());
    e.on_timeout();
    assert_eq!(e.buf().contents(), "abcj");
    assert_eq!(e.mode, Mode::Insert);
}

#[test]
fn writes_files() {
    let dir = std::env::temp_dir().join(format!("nib-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.md");
    std::fs::write(&path, "line1\r\nline2\r\n").unwrap();
    let mut e = Editor::new(80, 24);
    e.open_initial(std::slice::from_ref(&path));
    assert!(e.buf().is_markdown());
    assert_eq!(e.buf().contents(), "line1\nline2");
    feed(&mut e, "ddix<Esc>:w<CR>");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xline2\r\n");
    assert!(!e.buf().is_modified());
    feed(&mut e, "ddix<Esc>:q<CR>");
    assert!(!e.quit);
    assert!(e.messages[0].0.starts_with("E37"));
    feed(&mut e, ":q!<CR>");
    assert!(e.quit);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn markdown_list_continuation() {
    let mut e = md("- item");
    feed(&mut e, "A<CR>next<Esc>");
    assert_eq!(e.buf().contents(), "- item\n- next");
    let mut e = md("1. one");
    feed(&mut e, "A<CR>two<CR>three<Esc>");
    assert_eq!(e.buf().contents(), "1. one\n2. two\n3. three");
    let mut e = md("- [x] done");
    feed(&mut e, "A<CR>todo<Esc>");
    assert_eq!(e.buf().contents(), "- [x] done\n- [ ] todo");
    let mut e = md("- a");
    feed(&mut e, "A<CR><CR>text<Esc>");
    assert_eq!(e.buf().contents(), "- a\ntext");
    let mut e = md("> quote");
    feed(&mut e, "ofoo<Esc>");
    assert_eq!(e.buf().contents(), "> quote\n> foo");
    let mut e = md("- a");
    feed(&mut e, "A<CR><Tab>b<Esc>");
    assert_eq!(e.buf().contents(), "- a\n    - b");
    let mut e = md("1. a\n2. b");
    feed(&mut e, "ox<Esc>");
    assert_eq!(e.buf().contents(), "1. a\n2. x\n3. b");
    // Not continued in non-markdown buffers.
    assert_eq!(run("- a", "A<CR>b<Esc>"), "- a\nb");
}

#[test]
fn unnamed_buffers_are_markdown() {
    let e = Editor::new(80, 24);
    assert!(e.buf().is_markdown());
    let mut e = Editor::new(80, 24);
    feed(&mut e, ":enew<CR>");
    assert!(e.buf().is_markdown());
    let dir = std::env::temp_dir().join(format!("nib-ft-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let txt = dir.join("notes.txt");
    feed(&mut e, &format!(":w {}<CR>", txt.display()));
    assert!(!e.buf().is_markdown(), "saving with a non-markdown extension switches to text");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn markdown_task_toggle() {
    let mut e = md("- [ ] task");
    feed(&mut e, " x");
    assert_eq!(e.buf().contents(), "- [x] task");
    feed(&mut e, " x");
    assert_eq!(e.buf().contents(), "- [ ] task");
}

#[test]
fn heading_folds() {
    let mut e = md("# A\na1\na2\n# B\nb1");
    feed(&mut e, "zc");
    assert!(e.buf().folds.contains(&0));
    feed(&mut e, "j");
    assert_eq!(cur(&e), (3, 0), "j skips the closed fold");
    feed(&mut e, "kzo");
    assert!(e.buf().folds.is_empty());
    feed(&mut e, "zMG");
    assert_eq!(cur(&e).0, 3, "G into a closed fold lands on the fold");
    feed(&mut e, "zR");
    assert!(e.buf().folds.is_empty());
    feed(&mut e, "ggzcdd");
    assert_eq!(e.buf().contents(), "# B\nb1");
}

#[test]
fn format_paragraph() {
    let mut e = ed("aaa bbb ccc ddd eee");
    feed(&mut e, ":set tw=8<CR>gqq");
    assert_eq!(e.buf().contents(), "aaa bbb\nccc ddd\neee");
    let mut e = md("- aaa bbb ccc ddd");
    feed(&mut e, ":set tw=10<CR>gqq");
    assert_eq!(e.buf().contents(), "- aaa bbb\n  ccc ddd");
}

#[test]
fn windows_and_tabs() {
    let mut e = ed("text");
    feed(&mut e, ":sp<CR>");
    assert_eq!(layout::leaves(&e.tab().root).len(), 2);
    let top = e.win_id();
    feed(&mut e, "<C-w>j");
    assert_ne!(e.win_id(), top);
    feed(&mut e, "<C-w>k");
    assert_eq!(e.win_id(), top);
    feed(&mut e, ":vs<CR>");
    assert_eq!(layout::leaves(&e.tab().root).len(), 3);
    feed(&mut e, "<C-w>l");
    feed(&mut e, "<C-w>o");
    assert_eq!(layout::leaves(&e.tab().root).len(), 1);
    feed(&mut e, ":tabnew<CR>");
    assert_eq!(e.tabs.len(), 2);
    assert_eq!(e.tab, 1);
    feed(&mut e, "gt");
    assert_eq!(e.tab, 0);
    feed(&mut e, "gt:tabc<CR>");
    assert_eq!(e.tabs.len(), 1);
    assert_eq!(e.buf().contents(), "text");
    // Editing in one window shows in the other.
    feed(&mut e, ":sp<CR>x<C-w>wu");
    assert_eq!(e.buf().contents(), "text");
    feed(&mut e, ":q<CR>");
    assert!(!e.quit);
    feed(&mut e, ":q<CR>");
    assert!(e.quit);
}

#[test]
fn buffers() {
    let mut e = ed("first");
    feed(&mut e, ":enew<CR>isecond<Esc>");
    assert_eq!(e.buffers.len(), 2);
    feed(&mut e, "<C-^>");
    assert_eq!(e.buf().contents(), "first");
    feed(&mut e, ":bn<CR>");
    assert_eq!(e.buf().contents(), "second");
    feed(&mut e, ":bd<CR>");
    assert!(e.messages[0].0.starts_with("E89"));
    feed(&mut e, ":bd!<CR>");
    assert_eq!(e.buffers.len(), 1);
    assert_eq!(e.buf().contents(), "first");
}

#[test]
fn scrolling_keeps_cursor_visible() {
    let text: String = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut e = ed(&text);
    feed(&mut e, "50G");
    let top = e.win().top;
    let h = e.text_rect(e.win_id()).h as usize;
    assert!(top <= 49 && 49 < top + h);
    feed(&mut e, "<C-d>");
    assert!(e.cursor().line > 49);
    feed(&mut e, "gg<C-f>");
    assert!(e.win().top > 0);
    feed(&mut e, "zt");
    assert_eq!(e.win().top + e.options.scrolloff, e.cursor().line, "zt respects scrolloff");
    feed(&mut e, "L");
    let vis = e.visible_line_list();
    assert!(vis.contains(&e.cursor().line));
}

#[test]
fn preview_reveal_range() {
    let mut e = md("| a | b |\n|---|---|\n| c | d |\n\ntext\n```\ncode\n```");
    feed(&mut e, "j");
    let w = e.win();
    assert_eq!(e.reveal_range(w), (0, 2), "whole table revealed");
    feed(&mut e, "Gk");
    let w = e.win();
    assert_eq!(e.reveal_range(w), (5, 7), "whole code block revealed");
    feed(&mut e, "gg4j");
    let w = e.win();
    assert_eq!(e.reveal_range(w), (4, 4));
}

#[test]
fn scaled_headings_layout() {
    let mut e = md("# Big\n## Medium\n##### Small\ntext\n");
    e.text_sizing = true;
    feed(&mut e, "G");
    let views = e.layout_window(e.win_id());
    assert_eq!(views[0].height(), 2, "H1 takes two rows");
    assert_eq!(views[1].height(), 2, "H2 takes two rows");
    assert_eq!(views[2].height(), 1, "H5 stays normal size");
    // Cursor on the heading shows it raw and normal-sized.
    feed(&mut e, "gg");
    let views = e.layout_window(e.win_id());
    assert_eq!(views[0].height(), 1);
    assert!(views[0].scale.is_none());
    // Mouse click on the second row of the scaled H2 maps to that line.
    feed(&mut e, "G");
    let tr = e.text_rect(e.win_id());
    let (_, p) = e.screen_to_pos(tr.x + 4, tr.y + 3).unwrap();
    assert_eq!(p.line, 1);
    // Turning the option off: plain-size headings, styled by weight only.
    feed(&mut e, ":set noheadingscale<CR>");
    let views = e.layout_window(e.win_id());
    assert!(views[0].scale.is_none());
    assert_eq!(views[0].rl.cells.iter().map(|c| c.ch).collect::<String>(), "Big");
    assert_eq!(views[0].height(), 1);
}

#[test]
fn readable_line_length() {
    let mut e = ed("x");
    e.resize(120, 24);
    feed(&mut e, ":set nonu<CR>");
    // Narrower than the window: a centred column.
    let tr = e.text_rect(e.win_id());
    assert_eq!((tr.x, tr.w), (15, 90));
    feed(&mut e, " >");
    assert_eq!(e.options.wrapwidth, 94);
    feed(&mut e, " < <");
    assert_eq!(e.options.wrapwidth, 86);
    // Toggle off (window edge) and back on to the same width.
    feed(&mut e, " w");
    assert_eq!(e.options.wrapwidth, 0);
    assert_eq!(e.text_rect(e.win_id()).w, 120);
    feed(&mut e, " w");
    assert_eq!(e.options.wrapwidth, 86);
    // Widening stops at the window edge; there is a sensible minimum.
    feed(&mut e, ":wrapwidth 500<CR>");
    assert_eq!(e.text_rect(e.win_id()).w, 120);
    feed(&mut e, ":wrapwidth +4<CR>");
    assert_eq!(e.options.wrapwidth, 120);
    feed(&mut e, ":wrapwidth 3<CR>");
    assert_eq!(e.options.wrapwidth, 20);
    // A count before the keybind is not mistaken for a range near the end.
    feed(&mut e, "3 >");
    assert_eq!(e.options.wrapwidth, 24);
    // No readable width without wrapping.
    feed(&mut e, ":set nowrap<CR>");
    assert_eq!(e.text_rect(e.win_id()).w, 120);
}

#[test]
fn wrapped_rows_hang_under_the_text() {
    let words = "word ".repeat(12);
    let mut e = md(&format!("- [ ] {words}\n> {words}\n  {words}\n{words}\n"));
    feed(&mut e, ":wrapwidth 30<CR>G");
    let views = e.layout_window(e.win_id());
    let indent = |i: usize| views[i].indent.iter().map(|c| c.ch).collect::<String>();
    assert_eq!(indent(0), "  ", "under the task text, past the checkbox");
    assert_eq!(indent(1), "▎ ", "quote bar carries on");
    assert_eq!(indent(2), "  ");
    assert_eq!(indent(3), "");
    assert!(views[0].rows.len() > 1);
    // Cursor and clicks on a continuation row account for the indent.
    let lv = &views[0];
    let col = lv.rl.cells[lv.rows[1].start].src.unwrap();
    assert_eq!(Editor::locate_col(lv, col), (1, 2));
    assert_eq!(Editor::col_at(lv, 1, 2), col);
    assert_eq!(Editor::col_at(lv, 1, 0), col, "clicking the indent lands on the text");
    // Raw source (the cursor line) hangs under the text after `- [ ] `.
    feed(&mut e, "gg");
    let views = e.layout_window(e.win_id());
    assert_eq!(views[0].indent.len(), 6);
    feed(&mut e, ":set nobreakindent<CR>");
    assert!(e.layout_window(e.win_id())[0].indent.is_empty());
}
