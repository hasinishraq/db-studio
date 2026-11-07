import { useEffect, useRef } from 'react';
import * as monaco from 'monaco-editor';
import EditorWorker from 'monaco-editor/editor/editor.worker.js?worker';
import type { ColumnInfo } from '../../bindings';

window.MonacoEnvironment = { getWorker: () => new EditorWorker() };

monaco.editor.defineTheme('studio-dark', {
  base: 'vs-dark',
  inherit: true,
  rules: [],
  colors: { 'editor.background': '#1b2733', 'editorGutter.background': '#1b2733' },
});
monaco.editor.defineTheme('studio-light', {
  base: 'vs',
  inherit: true,
  rules: [],
  colors: { 'editor.background': '#ffffff' },
});

/** Completion source, replaced whenever the active connection's schema loads. */
let schemaColumns: ColumnInfo[] = [];

monaco.languages.registerCompletionItemProvider('sql', {
  provideCompletionItems(model, position) {
    const word = model.getWordUntilPosition(position);
    const range = {
      startLineNumber: position.lineNumber,
      endLineNumber: position.lineNumber,
      startColumn: word.startColumn,
      endColumn: word.endColumn,
    };
    const kinds = monaco.languages.CompletionItemKind;
    const tables = [...new Set(schemaColumns.map((c) => c.table))].map((table) => ({
      label: table,
      kind: kinds.Class,
      insertText: table,
      range,
    }));
    const columns = schemaColumns.map((c) => ({
      label: c.name,
      detail: `${c.table}: ${c.dataType}`,
      kind: kinds.Field,
      insertText: c.name,
      range,
    }));
    return { suggestions: [...tables, ...columns] };
  },
});

export interface EditorHandle {
  selection: () => string;
  markError: (start: number, end: number, message: string) => void;
  clearMarks: () => void;
}

export function SqlEditor({
  value,
  onChange,
  onRun,
  columns,
  handle,
}: {
  value: string;
  onChange: (sql: string) => void;
  onRun: () => void;
  columns: ColumnInfo[];
  handle: React.RefObject<EditorHandle | null>;
}) {
  const host = useRef<HTMLDivElement>(null);
  const run = useRef(onRun);
  run.current = onRun;
  const change = useRef(onChange);
  change.current = onChange;

  useEffect(() => {
    schemaColumns = columns;
  }, [columns]);

  useEffect(() => {
    if (!host.current) return undefined;
    const theme = document.documentElement.dataset.theme === 'light' ? 'studio-light' : 'studio-dark';
    const editor = monaco.editor.create(host.current, {
      value,
      language: 'sql',
      theme,
      automaticLayout: true,
      minimap: { enabled: false },
      fontFamily: 'JetBrains Mono, monospace',
      fontSize: 13,
      scrollBeyondLastLine: false,
    });
    editor.onDidChangeModelContent(() => change.current(editor.getValue()));
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => run.current());
    handle.current = {
      selection: () => {
        const selection = editor.getSelection();
        const model = editor.getModel();
        return selection && model && !selection.isEmpty() ? model.getValueInRange(selection) : '';
      },
      markError: (start, end, message) => {
        const model = editor.getModel();
        if (!model) return;
        const from = model.getPositionAt(start);
        const to = model.getPositionAt(end);
        monaco.editor.setModelMarkers(model, 'run', [
          {
            severity: monaco.MarkerSeverity.Error,
            message,
            startLineNumber: from.lineNumber,
            startColumn: from.column,
            endLineNumber: to.lineNumber,
            endColumn: to.column,
          },
        ]);
      },
      clearMarks: () => {
        const model = editor.getModel();
        if (model) monaco.editor.setModelMarkers(model, 'run', []);
      },
    };
    return () => editor.dispose();
    // The editor is created once; later values come from the user typing in it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <div ref={host} className="h-full w-full" />;
}
