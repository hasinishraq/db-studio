import { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Bot,
  CircleAlert,
  CircleCheck,
  Code2,
  EyeOff,
  LifeBuoy,
  LoaderCircle,
  Minus,
  Plus,
  Search,
  SlidersHorizontal,
  Table2,
  Trash2,
  Workflow,
  type LucideIcon,
} from 'lucide-react';
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
import { Button, cn, ErrorNote, Input, Segmented, Select, Switch } from '../ui';
import { notify } from '../ui/toast';

type SectionId = 'general' | 'editor' | 'results' | 'behaviour' | 'assistant' | 'hidden' | 'help';

const SECTIONS: { id: SectionId; label: string; icon: LucideIcon; blurb: string }[] = [
  { id: 'general', label: 'General', icon: SlidersHorizontal, blurb: 'Appearance, text size and history.' },
  { id: 'editor', label: 'Editor', icon: Code2, blurb: 'How the SQL editor looks and helps while you type.' },
  { id: 'results', label: 'Results and queries', icon: Table2, blurb: 'Limits on running statements and how rows are shown.' },
  { id: 'behaviour', label: 'Behaviour', icon: Workflow, blurb: 'What happens after you connect and when you export.' },
  { id: 'assistant', label: 'Assistant', icon: Bot, blurb: 'The model the assistant uses and what it may see.' },
  { id: 'hidden', label: 'Hidden columns', icon: EyeOff, blurb: 'Per-connection rules for what the assistant may read.' },
  { id: 'help', label: 'Troubleshooting', icon: LifeBuoy, blurb: 'Information to include when you report a problem.' },
];

/** The search text, so each row can decide whether it matches. */
const SearchContext = createContext('');

function matches(query: string, ...texts: (string | undefined)[]): boolean {
  if (!query) return true;
  const needle = query.toLowerCase();
  return texts.some((text) => text?.toLowerCase().includes(needle));
}

/** One setting: what it is on the left, its control on the right. */
function Row({
  label,
  hint,
  children,
  stacked,
}: {
  label: string;
  hint?: string;
  /** Gets the id to put on the control, so the label names it. */
  children: (id: string) => ReactNode;
  /** Puts the control under the text, for wide controls. */
  stacked?: boolean;
}) {
  const id = useId();
  const query = useContext(SearchContext);
  if (!matches(query, label, hint)) return null;
  return (
    <div
      data-row
      className={cn(
        'gap-x-8 gap-y-2 px-4 py-3',
        stacked ? 'flex flex-col' : 'flex items-center justify-between',
      )}
    >
      <div className="min-w-0">
        <label htmlFor={id} className="block text-sm">
          {label}
        </label>
        {hint && <p className="mt-0.5 text-xs text-muted">{hint}</p>}
      </div>
      <div className={cn('shrink-0', stacked && 'w-full')}>{children(id)}</div>
    </div>
  );
}

/** A group of rows on one surface, split by hairlines. */
function Group({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <div className="[&:not(:has([data-row]))]:hidden">
      {title && <h3 className="mb-2 text-xs font-semibold text-muted">{title}</h3>}
      <div className="divide-y divide-line overflow-hidden rounded-md border border-line bg-panel">{children}</div>
    </div>
  );
}

function Section({ id, children }: { id: SectionId; children: ReactNode }) {
  const section = SECTIONS.find((s) => s.id === id);
  if (!section) return null;
  return (
    <section
      id={`settings-${id}`}
      aria-labelledby={`settings-${id}-title`}
      className="space-y-5 [&:not(:has([data-row]))]:hidden"
    >
      <header>
        <h2 id={`settings-${id}-title`} className="text-base font-semibold">
          {section.label}
        </h2>
        <p className="mt-0.5 text-sm text-muted">{section.blurb}</p>
      </header>
      {children}
    </section>
  );
}

function NumberInput({
  id,
  value,
  min,
  max,
  onChange,
  unit,
}: {
  id: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  unit?: string;
}) {
  return (
    <span className="flex items-center gap-2">
      <Input
        id={id}
        type="number"
        className="w-28 text-right tabular-nums"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      {unit && <span className="w-12 text-xs text-muted">{unit}</span>}
    </span>
  );
}

/** Three small window pictures, so the choice is seen rather than read. */
function ThemePicker({ value, onChange }: { value: Theme; onChange: (theme: Theme) => void }) {
  const options: { value: Theme; label: string; panes: [string, string] }[] = [
    { value: 'System', label: 'Match the system', panes: ['#232428', '#f5f6f8'] },
    { value: 'Dark', label: 'Dark', panes: ['#232428', '#1c1d20'] },
    { value: 'Light', label: 'Light', panes: ['#f5f6f8', '#ffffff'] },
  ];
  return (
    <div role="radiogroup" aria-label="Theme" className="grid grid-cols-3 gap-3">
      {options.map((option) => {
        const chosen = value === option.value;
        return (
          <label key={option.value} className="group cursor-pointer">
            <input
              type="radio"
              name="theme"
              className="peer sr-only"
              value={option.value}
              checked={chosen}
              onChange={() => onChange(option.value)}
            />
            <span
              aria-hidden
              className={cn(
                'flex h-16 overflow-hidden rounded-md border-2 transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-accent',
                chosen ? 'border-accent' : 'border-line group-hover:border-edge',
              )}
            >
              <span className="w-1/3" style={{ background: option.panes[0] }} />
              <span className="flex flex-1 flex-col gap-1 p-2" style={{ background: option.panes[1] }}>
                <span className="h-1.5 w-3/4 rounded-full bg-[#7a8190]/50" />
                <span className="h-1.5 w-1/2 rounded-full bg-[#7a8190]/35" />
                <span className="mt-auto h-1.5 w-1/3 rounded-full" style={{ background: '#3d7bf2' }} />
              </span>
            </span>
            <span className={cn('mt-1.5 block text-xs', chosen ? 'text-ink' : 'text-muted')}>{option.label}</span>
          </label>
        );
      })}
    </div>
  );
}

function MaskingEditor() {
  const { profiles } = useStudio();
  const client = useQueryClient();
  const query = useContext(SearchContext);
  const [profileId, setProfileId] = useState('');
  const [rules, setRules] = useState<MaskingRule[]>([]);
  const profile = profiles.find((p) => p.id === profileId);

  useEffect(() => {
    if (profileId) void api.maskingList(profileId).then(setRules);
  }, [profileId]);

  const update = (index: number, change: Partial<MaskingRule>) =>
    setRules((all) => all.map((r, i) => (i === index ? { ...r, ...change } : r)));

  return (
    <>
      <Group>
        <Row label="Connection" hint="Rules apply to one connection at a time.">
          {(id) => (
            <Select id={id} className="w-56" value={profileId} onChange={(e) => setProfileId(e.target.value)}>
              <option value="">Choose a connection</option>
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          )}
        </Row>
        {profile && (
          <Row label="What the assistant may do">
            {(id) => (
              <Select
                id={id}
                className="w-56"
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
            )}
          </Row>
        )}
      </Group>
      {profile && matches(query, 'hidden columns', 'hide a column', 'schema table column') && (
        <div data-row>
          <h3 className="mb-2 text-xs font-semibold text-muted">Columns hidden from the assistant</h3>
          <div className="overflow-hidden rounded-md border border-line bg-panel">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-line text-left text-xs text-muted">
                  <th className="px-3 py-2 font-medium">Schema</th>
                  <th className="px-3 py-2 font-medium">Table</th>
                  <th className="px-3 py-2 font-medium">Column</th>
                  <th className="w-10" />
                </tr>
              </thead>
              <tbody>
                {rules.length === 0 && (
                  <tr>
                    <td colSpan={4} className="px-3 py-4 text-center text-xs text-muted">
                      No columns hidden yet. Password-like columns are always hidden.
                    </td>
                  </tr>
                )}
                {rules.map((rule, index) => (
                  <tr key={index} className="border-b border-line last:border-0">
                    <td className="px-2 py-1.5">
                      <Input aria-label="Schema" placeholder="Schema" value={rule.schemaName} onChange={(e) => update(index, { schemaName: e.target.value })} />
                    </td>
                    <td className="px-2 py-1.5">
                      <Input aria-label="Table" placeholder="Table" value={rule.tableName} onChange={(e) => update(index, { tableName: e.target.value })} />
                    </td>
                    <td className="px-2 py-1.5">
                      <Input aria-label="Column" placeholder="Column" value={rule.columnName} onChange={(e) => update(index, { columnName: e.target.value })} />
                    </td>
                    <td className="pr-2 text-right">
                      <Button
                        tone="quiet"
                        className="w-[26px] px-0 hover:text-danger"
                        aria-label="Remove"
                        title="Remove"
                        onClick={() => setRules((all) => all.filter((_, i) => i !== index))}
                      >
                        <Trash2 size={14} aria-hidden />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-2 text-xs text-muted">Hidden columns show as *** to the model. Password-like columns are always hidden.</p>
          <div className="mt-3 flex gap-2">
            <Button onClick={() => setRules((all) => [...all, { schemaName: '', tableName: '', columnName: '' }])}>
              <Plus size={14} aria-hidden /> Hide a column
            </Button>
            <Button tone="primary" onClick={() => void api.maskingSet(profile.id, rules)}>
              Save hidden columns
            </Button>
          </div>
        </div>
      )}
    </>
  );
}

type SaveState = 'idle' | 'saving' | 'saved' | 'failed';

function SaveStatus({ state }: { state: SaveState }) {
  const view = {
    idle: { text: 'Changes save as you make them.', icon: null, tone: 'text-muted' },
    saving: { text: 'Saving', icon: <LoaderCircle size={13} className="animate-spin" />, tone: 'text-muted' },
    saved: { text: 'Saved', icon: <CircleCheck size={13} className="text-safe" />, tone: 'text-muted' },
    failed: { text: 'Not saved', icon: <CircleAlert size={13} />, tone: 'text-danger' },
  }[state];
  return (
    <p role="status" className={cn('flex items-center gap-1.5 text-xs', view.tone)}>
      {view.icon}
      {view.text}
    </p>
  );
}

export function SettingsTab() {
  const client = useQueryClient();
  const zoom = useZoom();
  const { data } = useQuery({ queryKey: ['settings'], queryFn: api.settingsGet });
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [apiKey, setApiKey] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<SectionId>('general');
  const [search, setSearch] = useState('');
  const [empty, setEmpty] = useState(false);
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null);
  const body = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (data && !settings) setSettings(data);
  }, [data, settings]);
  useEffect(() => () => {
    if (pending.current) clearTimeout(pending.current);
  }, []);
  // A search that matches nothing says so, rather than leaving a blank page.
  useLayoutEffect(() => {
    setEmpty(Boolean(search) && !body.current?.querySelector('[data-row]'));
  });
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
      notify.success('The key is in the system keychain.');
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const testProvider = async () => {
    setError(null);
    try {
      await persist(settings);
      notify.success(`The model replied: ${await api.aiTestProvider()}`);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const copyDiagnostics = async () => {
    const report = await api.diagnostics();
    const text = [`DB Studio ${report.version}`, report.os, ...report.logTail].join('\n');
    await navigator.clipboard.writeText(text);
    notify.success('Diagnostics copied.');
  };

  /** Searching shows every section, so a match is found wherever it lives. */
  const shown = (id: SectionId) => Boolean(search) || active === id;

  return (
    <SearchContext.Provider value={search.trim()}>
      <div className="flex h-full">
        <nav aria-label="Settings sections" className="flex w-56 shrink-0 select-none flex-col border-r border-line bg-panel">
          <div className="p-2">
            <div className="relative">
              <Search size={13} aria-hidden className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-muted" />
              <Input
                aria-label="Search settings"
                placeholder="Search settings"
                className="pl-7"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
          </div>
          <div className="flex-1 space-y-px overflow-auto px-2">
            {SECTIONS.map((section) => {
              const Icon = section.icon;
              const current = !search && active === section.id;
              return (
                <button
                  key={section.id}
                  type="button"
                  aria-current={current ? 'page' : undefined}
                  className={cn(
                    'flex h-7 w-full items-center gap-2 rounded px-2 text-left text-sm',
                    current ? 'bg-pick text-ink' : 'text-ink/85 hover:bg-hover',
                  )}
                  onClick={() => {
                    setSearch('');
                    setActive(section.id);
                    body.current?.scrollTo?.({ top: 0 });
                  }}
                >
                  <Icon size={14} aria-hidden className={current ? 'text-accent' : 'text-muted'} />
                  {section.label}
                </button>
              );
            })}
          </div>
          <div className="border-t border-line px-3 py-2">
            <SaveStatus state={saveState} />
          </div>
        </nav>

        <div ref={body} className="min-w-0 flex-1 overflow-auto bg-ground">
          <div className="mx-auto max-w-2xl space-y-10 px-8 py-8">
            <ErrorNote message={error} />
            {empty && <p className="py-10 text-center text-sm text-muted">No settings match "{search}".</p>}

            {shown('general') && (
              <Section id="general">
                <Group title="Appearance">
                  <Row label="Theme" stacked>
                    {() => <ThemePicker value={settings.theme ?? 'System'} onChange={(theme) => set({ theme })} />}
                  </Row>
                  <Row label="Text size" hint="Ctrl + and Ctrl - work anywhere in the app.">
                    {(id) => (
                      <span className="flex items-center gap-1">
                        <Button className="w-[26px] px-0" aria-label="Make everything smaller" onClick={() => changeZoom('out')}>
                          <Minus size={13} aria-hidden />
                        </Button>
                        <output id={id} className="w-12 text-center text-sm tabular-nums">
                          {Math.round(zoom * 100)}%
                        </output>
                        <Button className="w-[26px] px-0" aria-label="Make everything bigger" onClick={() => changeZoom('in')}>
                          <Plus size={13} aria-hidden />
                        </Button>
                        <Button tone="quiet" className="ml-1" onClick={() => changeZoom('reset')}>
                          Reset
                        </Button>
                      </span>
                    )}
                  </Row>
                </Group>
                <Group title="Data">
                  <Row label="Runs kept in history">
                    {(id) => (
                      <NumberInput
                        id={id}
                        value={settings.historyLimit ?? 1000}
                        min={1}
                        max={100000}
                        unit="runs"
                        onChange={(historyLimit) => set({ historyLimit })}
                      />
                    )}
                  </Row>
                  <Row label="Hide masked columns in results too" hint="Otherwise they are hidden only from the assistant.">
                    {(id) => (
                      <Switch id={id} checked={settings.maskInGrid ?? false} onChange={(maskInGrid) => set({ maskInGrid })} />
                    )}
                  </Row>
                </Group>
              </Section>
            )}

            {shown('editor') && (
              <Section id="editor">
                <Group title="Text">
                  <Row label="Font size">
                    {(id) => (
                      <NumberInput
                        id={id}
                        value={prefs.editorFontSize}
                        min={8}
                        max={32}
                        unit="px"
                        onChange={(editorFontSize) => set({ editorFontSize })}
                      />
                    )}
                  </Row>
                  <Row label="Spaces per tab">
                    {(id) => (
                      <NumberInput
                        id={id}
                        value={prefs.editorTabSize}
                        min={1}
                        max={8}
                        unit="spaces"
                        onChange={(editorTabSize) => set({ editorTabSize })}
                      />
                    )}
                  </Row>
                  <Row label="Wrap long lines">
                    {(id) => (
                      <Switch id={id} checked={prefs.editorWordWrap} onChange={(editorWordWrap) => set({ editorWordWrap })} />
                    )}
                  </Row>
                </Group>
                <Group title="Display and help">
                  <Row label="Show line numbers">
                    {(id) => (
                      <Switch
                        id={id}
                        checked={prefs.editorLineNumbers}
                        onChange={(editorLineNumbers) => set({ editorLineNumbers })}
                      />
                    )}
                  </Row>
                  <Row label="Show the overview map" hint="A small picture of the whole query at the right edge.">
                    {(id) => (
                      <Switch id={id} checked={prefs.editorMinimap} onChange={(editorMinimap) => set({ editorMinimap })} />
                    )}
                  </Row>
                  <Row label="Suggest table and column names while typing">
                    {(id) => (
                      <Switch
                        id={id}
                        checked={prefs.editorSuggestions}
                        onChange={(editorSuggestions) => set({ editorSuggestions })}
                      />
                    )}
                  </Row>
                </Group>
              </Section>
            )}

            {shown('results') && (
              <Section id="results">
                <Group title="Limits">
                  <Row label="Stop a statement after" hint="0 means never. The assistant keeps its own shorter limit.">
                    {(id) => (
                      <NumberInput
                        id={id}
                        value={settings.statementTimeoutSecs ?? 0}
                        min={0}
                        max={86400}
                        unit="seconds"
                        onChange={(statementTimeoutSecs) => set({ statementTimeoutSecs })}
                      />
                    )}
                  </Row>
                  <Row label="Stop reading a result after" hint="0 means no limit. Exports always write every row.">
                    {(id) => (
                      <NumberInput
                        id={id}
                        value={settings.rowLimit ?? 0}
                        min={0}
                        max={100000000}
                        unit="rows"
                        onChange={(rowLimit) => set({ rowLimit })}
                      />
                    )}
                  </Row>
                </Group>
                <Group title="Grid">
                  <Row label="Show empty values as">
                    {(id) => (
                      <Input
                        id={id}
                        className="w-28 font-mono text-xs"
                        value={prefs.nullText}
                        maxLength={16}
                        onChange={(e) => set({ nullText: e.target.value })}
                      />
                    )}
                  </Row>
                  <Row label="Row height">
                    {() => (
                      <Segmented<RowDensity>
                        label="Row height"
                        value={prefs.rowDensity}
                        onChange={(rowDensity) => set({ rowDensity })}
                        options={[
                          { value: 'Compact', label: 'Compact' },
                          { value: 'Roomy', label: 'Roomy' },
                        ]}
                      />
                    )}
                  </Row>
                </Group>
              </Section>
            )}

            {shown('behaviour') && (
              <Section id="behaviour">
                <Group>
                  <Row label="Open a query tab after connecting">
                    {(id) => (
                      <Switch
                        id={id}
                        checked={prefs.openQueryOnConnect}
                        onChange={(openQueryOnConnect) => set({ openQueryOnConnect })}
                      />
                    )}
                  </Row>
                  <Row label="Export button format" hint="The format the Export button writes. Others stay in its menu.">
                    {(id) => (
                      <Select
                        id={id}
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
                    )}
                  </Row>
                </Group>
              </Section>
            )}

            {shown('assistant') && (
              <Section id="assistant">
                <Group title="Model">
                  <Row label="Provider">
                    {(id) => (
                      <Select
                        id={id}
                        className="w-56"
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
                    )}
                  </Row>
                  <Row label="Endpoint">
                    {(id) => (
                      <Input
                        id={id}
                        className="w-72 font-mono text-xs"
                        value={settings.aiEndpoint}
                        onChange={(e) => set({ aiEndpoint: e.target.value })}
                      />
                    )}
                  </Row>
                  <Row label="Model">
                    {(id) => (
                      <Input
                        id={id}
                        className="w-72 font-mono text-xs"
                        value={settings.aiModel}
                        onChange={(e) => set({ aiModel: e.target.value })}
                      />
                    )}
                  </Row>
                  <Row label="API key" hint="Stored in the system keychain and never shown again.">
                    {(id) => (
                      <span className="flex gap-1.5">
                        <Input
                          id={id}
                          className="w-52"
                          type="password"
                          autoComplete="off"
                          placeholder="Paste a new key"
                          value={apiKey}
                          onChange={(e) => setApiKey(e.target.value)}
                        />
                        <Button disabled={!apiKey} onClick={() => void saveKey()}>
                          Save key
                        </Button>
                      </span>
                    )}
                  </Row>
                  <Row label="Check the model" hint="Sends a short test message with the settings above.">
                    {() => <Button onClick={() => void testProvider()}>Test the model</Button>}
                  </Row>
                </Group>
                <Group title="New chats">
                  <Row
                    label="Run safe reads without asking"
                    hint="Only statements the safety check calls read-only, with a time and row limit. Each chat can change it."
                  >
                    {(id) => (
                      <Switch id={id} checked={settings.autoRunReads ?? true} onChange={(autoRunReads) => set({ autoRunReads })} />
                    )}
                  </Row>
                  <Row
                    label="Let the assistant see result rows"
                    hint="Off by default. Masked columns stay hidden, and a connection set to never share rows still never shares them."
                  >
                    {(id) => (
                      <Switch id={id} checked={settings.aiShareRows ?? false} onChange={(aiShareRows) => set({ aiShareRows })} />
                    )}
                  </Row>
                </Group>
              </Section>
            )}

            {shown('hidden') && (
              <Section id="hidden">
                <MaskingEditor />
              </Section>
            )}

            {shown('help') && (
              <Section id="help">
                <Group>
                  <Row
                    label="Copy diagnostics"
                    hint="The app version, your system and recent log lines, ready to paste into a bug report."
                  >
                    {() => <Button onClick={() => void copyDiagnostics()}>Copy diagnostics</Button>}
                  </Row>
                </Group>
              </Section>
            )}
          </div>
        </div>
      </div>
    </SearchContext.Provider>
  );
}
