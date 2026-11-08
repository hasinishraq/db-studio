import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, errorMessage } from '../../api';
import { changeZoom, useZoom } from '../../api/zoom';
import { EXPORT_LABELS, prefsOf } from '../../api/settings';
import type {
  AiPolicy,
  AiProviderKind,
  ExportFormat,
  MaskingRule,
  RowDensity,
  Settings,
  Theme,
} from '../../bindings';
import { useStudio } from '../../context/Studio';
import { applyTheme } from '../../lib/theme';
import { Button, ErrorNote, Field, Input, Select } from '../ui';

function MaskingEditor() {
  const { profiles } = useStudio();
  const client = useQueryClient();
  const [profileId, setProfileId] = useState('');
  const [rules, setRules] = useState<MaskingRule[]>([]);
  const profile = profiles.find((p) => p.id === profileId);

  useEffect(() => {
    if (profileId) void api.maskingList(profileId).then(setRules);
  }, [profileId]);

  const update = (index: number, change: Partial<MaskingRule>) =>
    setRules((all) => all.map((r, i) => (i === index ? { ...r, ...change } : r)));

  return (
    <section className="mt-6">
      <h2 className="mb-2 text-lg font-bold">Per-connection assistant rules</h2>
      <Field label="Connection">
        <Select value={profileId} onChange={(e) => setProfileId(e.target.value)}>
          <option value="">Choose a connection</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>
      {profile && (
        <div className="mt-2 space-y-2">
          <Field label="What the assistant may do">
            <Select
              value={profile.aiPolicy}
              onChange={(e) => {
                const updated = { ...profile, aiPolicy: e.target.value as AiPolicy };
                void api.saveProfile(updated, null).then(() => client.invalidateQueries({ queryKey: ['profiles'] }));
              }}
            >
              <option value="Default">Follow the global settings</option>
              <option value="LocalOnly">Use a local model only</option>
              <option value="NoRowSharing">Never send rows to the model</option>
              <option value="Disabled">Turn the assistant off</option>
            </Select>
          </Field>
          <p className="text-sm text-muted">Hidden columns show as *** to the model. Password-like columns are always hidden.</p>
          {rules.map((rule, index) => (
            <div key={index} className="flex gap-2">
              <Input aria-label="Schema" placeholder="Schema" value={rule.schemaName} onChange={(e) => update(index, { schemaName: e.target.value })} />
              <Input aria-label="Table" placeholder="Table" value={rule.tableName} onChange={(e) => update(index, { tableName: e.target.value })} />
              <Input aria-label="Column" placeholder="Column" value={rule.columnName} onChange={(e) => update(index, { columnName: e.target.value })} />
              <Button onClick={() => setRules((all) => all.filter((_, i) => i !== index))}>Remove</Button>
            </div>
          ))}
          <div className="flex gap-2">
            <Button onClick={() => setRules((all) => [...all, { schemaName: '', tableName: '', columnName: '' }])}>Hide a column</Button>
            <Button tone="primary" onClick={() => void api.maskingSet(profile.id, rules)}>
              Save hidden columns
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}

const SECTIONS = [
  { id: 'general', label: 'General' },
  { id: 'editor', label: 'Editor' },
  { id: 'results', label: 'Results and queries' },
  { id: 'behaviour', label: 'Behaviour' },
  { id: 'assistant', label: 'Assistant' },
  { id: 'hidden', label: 'Hidden columns' },
] as const;

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={`settings-${id}`} aria-labelledby={`settings-${id}-title`} className="space-y-3 border-b border-line pb-6">
      <h2 id={`settings-${id}-title`} className="pt-2 text-lg font-bold">
        {title}
      </h2>
      {children}
    </section>
  );
}

function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  hint?: string;
}) {
  return (
    <label className="flex items-start gap-2 text-sm">
      <input type="checkbox" className="mt-0.5" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>
        {label}
        {hint && <span className="block text-muted">{hint}</span>}
      </span>
    </label>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  hint,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  hint?: string;
  onChange: (value: number) => void;
}) {
  return (
    <Field label={label}>
      <Input
        type="number"
        className="w-40"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      {hint && <span className="mt-1 block text-xs text-muted">{hint}</span>}
    </Field>
  );
}

type SaveState = 'idle' | 'saving' | 'saved' | 'failed';

export function SettingsTab() {
  const client = useQueryClient();
  const zoom = useZoom();
  const { data } = useQuery({ queryKey: ['settings'], queryFn: api.settingsGet });
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [apiKey, setApiKey] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<string | null>(null);
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (data && !settings) setSettings(data);
  }, [data, settings]);
  useEffect(() => () => {
    if (pending.current) clearTimeout(pending.current);
  }, []);
  if (!settings) return null;
  const prefs = prefsOf(settings);
  const provider = settings.aiProvider === 'Ollama' ? 'ollama' : 'openai';

  const persist = async (next: Settings) => {
    setSaveState('saving');
    try {
      await api.settingsSet(next);
      setError(null);
      setSaveState('saved');
      await client.invalidateQueries({ queryKey: ['settings'] });
    } catch (e) {
      setError(errorMessage(e));
      setSaveState('failed');
    }
  };

  /** Every change saves on its own, shortly after typing stops. */
  const set = (change: Partial<Settings>) => {
    const next = { ...settings, ...change };
    setSettings(next);
    if (change.theme) applyTheme(change.theme);
    if (pending.current) clearTimeout(pending.current);
    pending.current = setTimeout(() => void persist(next), 400);
  };

  const saveKey = async () => {
    setError(null);
    try {
      await api.setSecret({ kind: 'aiKey', provider }, apiKey);
      setApiKey('');
      setMessage('The key is in the system keychain.');
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const testProvider = async () => {
    setError(null);
    setMessage(null);
    try {
      await persist(settings);
      setMessage(`The model replied: ${await api.aiTestProvider()}`);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const copyDiagnostics = async () => {
    const report = await api.diagnostics();
    const text = [`DB Studio ${report.version}`, report.os, ...report.logTail].join('\n');
    await navigator.clipboard.writeText(text);
    setDiagnostics('Copied.');
  };

  const status =
    saveState === 'saving' ? 'Saving' : saveState === 'saved' ? 'Saved' : saveState === 'failed' ? 'Not saved' : '';

  return (
    <div className="flex h-full">
      <nav aria-label="Settings sections" className="w-48 shrink-0 space-y-0.5 border-r border-line bg-panel p-2">
        {SECTIONS.map((section) => (
          <button
            key={section.id}
            type="button"
            className="block w-full rounded px-2 py-1.5 text-left text-sm hover:bg-raised"
            onClick={() => document.getElementById(`settings-${section.id}`)?.scrollIntoView({ behavior: 'smooth' })}
          >
            {section.label}
          </button>
        ))}
        <p role="status" className="px-2 pt-3 text-xs text-muted">
          {status || 'Changes save as you make them.'}
        </p>
      </nav>
      <div className="min-w-0 flex-1 overflow-auto p-4">
        <div className="max-w-xl space-y-6">
          <ErrorNote message={error} />
          <Section id="general" title="General">
            <Field label="Theme">
              <Select value={settings.theme} onChange={(e) => set({ theme: e.target.value as Theme })}>
                <option value="System">Match the system</option>
                <option value="Dark">Dark</option>
                <option value="Light">Light</option>
              </Select>
            </Field>
            <div className="text-sm">
              <span className="mb-1 block text-muted">Text size (Ctrl + and Ctrl -)</span>
              <div className="flex items-center gap-2">
                <Button aria-label="Make everything smaller" onClick={() => changeZoom('out')}>
                  Smaller
                </Button>
                <output className="w-12 text-center">{Math.round(zoom * 100)}%</output>
                <Button aria-label="Make everything bigger" onClick={() => changeZoom('in')}>
                  Bigger
                </Button>
                <Button onClick={() => changeZoom('reset')}>Reset</Button>
              </div>
            </div>
            <NumberField
              label="Runs kept in history"
              value={settings.historyLimit ?? 1000}
              min={1}
              max={100000}
              onChange={(historyLimit) => set({ historyLimit })}
            />
            <Toggle
              checked={settings.maskInGrid ?? false}
              onChange={(maskInGrid) => set({ maskInGrid })}
              label="Hide masked columns in results too"
              hint="Otherwise they are hidden only from the assistant."
            />
          </Section>

          <Section id="editor" title="Editor">
            <NumberField
              label="Font size"
              value={prefs.editorFontSize}
              min={8}
              max={32}
              onChange={(editorFontSize) => set({ editorFontSize })}
            />
            <NumberField
              label="Spaces per tab"
              value={prefs.editorTabSize}
              min={1}
              max={8}
              onChange={(editorTabSize) => set({ editorTabSize })}
            />
            <Toggle checked={prefs.editorWordWrap} onChange={(editorWordWrap) => set({ editorWordWrap })} label="Wrap long lines" />
            <Toggle
              checked={prefs.editorLineNumbers}
              onChange={(editorLineNumbers) => set({ editorLineNumbers })}
              label="Show line numbers"
            />
            <Toggle
              checked={prefs.editorMinimap}
              onChange={(editorMinimap) => set({ editorMinimap })}
              label="Show the overview map"
              hint="A small picture of the whole query at the right edge."
            />
            <Toggle
              checked={prefs.editorSuggestions}
              onChange={(editorSuggestions) => set({ editorSuggestions })}
              label="Suggest table and column names while typing"
            />
          </Section>

          <Section id="results" title="Results and queries">
            <NumberField
              label="Stop a statement after (seconds)"
              value={settings.statementTimeoutSecs ?? 0}
              min={0}
              max={86400}
              hint="0 means never. The assistant keeps its own shorter limit."
              onChange={(statementTimeoutSecs) => set({ statementTimeoutSecs })}
            />
            <NumberField
              label="Stop reading a result after (rows)"
              value={settings.rowLimit ?? 0}
              min={0}
              max={100000000}
              hint="0 means no limit. Exports always write every row."
              onChange={(rowLimit) => set({ rowLimit })}
            />
            <Field label="Show empty values as">
              <Input className="w-40" value={prefs.nullText} maxLength={16} onChange={(e) => set({ nullText: e.target.value })} />
            </Field>
            <Field label="Row height">
              <Select
                className="w-40"
                value={prefs.rowDensity}
                onChange={(e) => set({ rowDensity: e.target.value as RowDensity })}
              >
                <option value="Compact">Compact</option>
                <option value="Roomy">Roomy</option>
              </Select>
            </Field>
          </Section>

          <Section id="behaviour" title="Behaviour">
            <Toggle
              checked={prefs.openQueryOnConnect}
              onChange={(openQueryOnConnect) => set({ openQueryOnConnect })}
              label="Open a query tab after connecting"
            />
            <Field label="Export button format">
              <Select
                className="w-40"
                value={prefs.exportFormat}
                onChange={(e) => set({ exportFormat: e.target.value as ExportFormat })}
              >
                {(Object.keys(EXPORT_LABELS) as ExportFormat[]).map((format) => (
                  <option key={format} value={format}>
                    {EXPORT_LABELS[format]}
                  </option>
                ))}
              </Select>
            </Field>
          </Section>

          <Section id="assistant" title="Assistant">
            <Field label="Provider">
              <Select
                value={settings.aiProvider}
                onChange={(e) => {
                  const aiProvider = e.target.value as AiProviderKind;
                  set({
                    aiProvider,
                    aiEndpoint: aiProvider === 'Ollama' ? 'http://localhost:11434' : 'https://api.openai.com/v1',
                  });
                }}
              >
                <option value="Ollama">Ollama</option>
                <option value="OpenAi">OpenAI or compatible</option>
              </Select>
            </Field>
            <Field label="Endpoint">
              <Input value={settings.aiEndpoint} onChange={(e) => set({ aiEndpoint: e.target.value })} />
            </Field>
            <Field label="Model">
              <Input value={settings.aiModel} onChange={(e) => set({ aiModel: e.target.value })} />
            </Field>
            <Field label="API key (stored in the system keychain, never shown again)">
              <div className="flex gap-2">
                <Input type="password" autoComplete="off" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
                <Button disabled={!apiKey} onClick={() => void saveKey()}>
                  Save key
                </Button>
              </div>
            </Field>
            <Toggle
              checked={settings.autoRunReads ?? true}
              onChange={(autoRunReads) => set({ autoRunReads })}
              label="Run safe reads without asking, in new chats"
              hint="Only statements the safety check calls read-only, with a time and row limit. Each chat can change it."
            />
            <Toggle
              checked={settings.aiShareRows ?? false}
              onChange={(aiShareRows) => set({ aiShareRows })}
              label="Let the assistant see result rows, in new chats"
              hint="Off by default. Masked columns stay hidden, and a connection set to never share rows still never shares them."
            />
            {message && <p className="text-sm">{message}</p>}
            <div className="flex gap-2">
              <Button onClick={() => void testProvider()}>Test the model</Button>
              <Button onClick={() => void copyDiagnostics()}>Copy diagnostics</Button>
              {diagnostics && <span className="self-center text-sm">{diagnostics}</span>}
            </div>
          </Section>

          <div id="settings-hidden">
            <MaskingEditor />
          </div>
        </div>
      </div>
    </div>
  );
}
