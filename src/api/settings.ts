import { useQuery } from '@tanstack/react-query';
import type { ExportFormat, RowDensity, Settings } from '../bindings';
import { api } from './index';

/** The preferences the screens read, with defaults filled in. Rust holds the saved copy. */
export interface Prefs {
  editorFontSize: number;
  editorTabSize: number;
  editorWordWrap: boolean;
  editorLineNumbers: boolean;
  editorMinimap: boolean;
  editorSuggestions: boolean;
  nullText: string;
  rowDensity: RowDensity;
  openQueryOnConnect: boolean;
  exportFormat: ExportFormat;
  aiAutoRun: boolean;
  aiShareRows: boolean;
}

export function prefsOf(settings: Settings | undefined): Prefs {
  return {
    editorFontSize: settings?.editorFontSize ?? 13,
    editorTabSize: settings?.editorTabSize ?? 2,
    editorWordWrap: settings?.editorWordWrap ?? false,
    editorLineNumbers: settings?.editorLineNumbers ?? true,
    editorMinimap: settings?.editorMinimap ?? false,
    editorSuggestions: settings?.editorSuggestions ?? true,
    nullText: settings?.nullText ?? 'NULL',
    rowDensity: settings?.rowDensity ?? 'Compact',
    openQueryOnConnect: settings?.openQueryOnConnect ?? false,
    exportFormat: settings?.exportFormat ?? 'Csv',
    aiAutoRun: settings?.autoRunReads ?? true,
    aiShareRows: settings?.aiShareRows ?? false,
  };
}

export function usePrefs(): Prefs {
  const { data } = useQuery({ queryKey: ['settings'], queryFn: api.settingsGet });
  return prefsOf(data);
}

export const EXPORT_LABELS: Record<ExportFormat, string> = {
  Csv: 'CSV',
  JsonLines: 'JSON lines',
  Xlsx: 'Excel',
  SqlInserts: 'SQL inserts',
};

/** Row height in pixels for each density. */
export const ROW_HEIGHTS: Record<RowDensity, number> = { Compact: 24, Roomy: 32 };
