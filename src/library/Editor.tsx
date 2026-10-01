// CodeMirror 6 editor for Library items (lazy-loaded with the Library screen).
import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";

// A tiny markdown + YAML-frontmatter mode: headings, emphasis, code, lists,
// links and frontmatter keys. Far smaller than the full markdown parser.
interface MdState {
  front: boolean;
  frontDone: boolean;
  fence: boolean;
}
const markdown = {
  name: "markdown-lite",
  startState: (): MdState => ({ front: false, frontDone: false, fence: false }),
  token(stream: import("@codemirror/language").StringStream, st: MdState): string | null {
    if (stream.sol()) {
      if (stream.match(/^---\s*$/)) {
        if (!st.frontDone && !st.front && stream.string === "---") {
          st.front = true;
          return "meta";
        }
        if (st.front) {
          st.front = false;
          st.frontDone = true;
          return "meta";
        }
      }
      if (stream.match(/^```/)) {
        st.fence = !st.fence;
        stream.skipToEnd();
        return "meta";
      }
    }
    if (st.fence) {
      stream.skipToEnd();
      return "monospace";
    }
    if (st.front) {
      if (stream.sol() && stream.match(/^[\w.-]+(?=:)/)) return "propertyName";
      if (stream.match(/^#.*/)) return "comment";
      stream.next();
      return "string";
    }
    st.frontDone = true;
    if (stream.sol() && stream.match(/^#{1,6}\s.*/)) return "heading";
    if (stream.sol() && stream.match(/^\s*([-*+]|\d+[.)])\s/)) return "list";
    if (stream.match(/^`[^`]*`/)) return "monospace";
    if (stream.match(/^\*\*[^*]+\*\*/)) return "strong";
    if (stream.match(/^\[[^\]]*\]\([^)]*\)/)) return "link";
    if (stream.match(/^<!--.*?-->/)) return "comment";
    stream.next();
    return null;
  },
};
import { tags } from "@lezer/highlight";

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "12.5px", backgroundColor: "var(--panel)", color: "var(--fg)" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
  ".cm-content": { padding: "12px 0", caretColor: "var(--accent)" },
  ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--faint)", border: "none", paddingLeft: "6px" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent) 6%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--muted)" },
  "&.cm-focused": { outline: "none" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": { backgroundColor: "color-mix(in srgb, var(--accent) 25%, transparent) !important" },
  ".cm-cursor": { borderLeftColor: "var(--accent)" },
});

const highlight = HighlightStyle.define([
  { tag: tags.heading, fontWeight: "600", color: "var(--accent)" },
  { tag: tags.strong, fontWeight: "600" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: [tags.monospace, tags.literal], color: "var(--local)" },
  { tag: [tags.url, tags.link], color: "var(--cloud)" },
  { tag: [tags.meta, tags.processingInstruction, tags.comment], color: "var(--faint)" },
  { tag: tags.list, color: "var(--muted)" },
  { tag: tags.propertyName, color: "var(--premium)" },
  { tag: tags.string, color: "var(--fg)" },
]);

export default function Editor({ value, onChange, onSave }: { value: string; onChange: (v: string) => void; onSave: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const cbs = useRef({ onChange, onSave });
  cbs.current = { onChange, onSave };

  useEffect(() => {
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          drawSelection(),
          highlightActiveLine(),
          EditorView.lineWrapping,
          StreamLanguage.define(markdown),
          syntaxHighlighting(highlight),
          theme,
          keymap.of([
            { key: "Mod-s", preventDefault: true, run: () => (cbs.current.onSave(), true) },
            indentWithTab,
            ...defaultKeymap,
            ...historyKeymap,
          ]),
          EditorView.updateListener.of((u) => u.docChanged && cbs.current.onChange(u.state.doc.toString())),
        ],
      }),
    });
    view.current = v;
    return () => v.destroy();
    // Recreated per item via `key`; external value changes are synced below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
  }, [value]);

  return <div ref={host} className="h-full overflow-hidden" />;
}
