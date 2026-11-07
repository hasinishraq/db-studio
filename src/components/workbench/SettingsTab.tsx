import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, errorMessage } from '../../api';
import { changeZoom, useZoom } from '../../api/zoom';
import type { AiPolicy, AiProviderKind, MaskingRule, Settings, Theme } from '../../bindings';
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

export function SettingsTab() {
  const client = useQueryClient();
  const zoom = useZoom();
  const { data } = useQuery({ queryKey: ['settings'], queryFn: api.settingsGet });
  const [settings, setSettings] = useState<Settings | null>(null);
  const [apiKey, setApiKey] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<string | null>(null);

  useEffect(() => {
    if (data) setSettings(data);
  }, [data]);
  if (!settings) return null;
  const set = (change: Partial<Settings>) => setSettings({ ...settings, ...change });
  const provider = settings.aiProvider === 'Ollama' ? 'ollama' : 'openai';

  const save = async () => {
    setError(null);
    try {
      await api.settingsSet(settings);
      if (apiKey) {
        await api.setSecret({ kind: 'aiKey', provider }, apiKey);
        setApiKey('');
      }
      applyTheme(settings.theme ?? 'System');
      await client.invalidateQueries({ queryKey: ['settings'] });
      setMessage('Saved.');
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const testProvider = async () => {
    setError(null);
    setMessage(null);
    try {
      await save();
      setMessage(`The model replied: ${await api.aiTestProvider()}`);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const copyDiagnostics = async () => {
    const report = await api.diagnostics();
    const text = [`AI DB Studio ${report.version}`, report.os, ...report.logTail].join('\n');
    await navigator.clipboard.writeText(text);
    setDiagnostics('Copied.');
  };

  return (
    <div className="h-full overflow-auto p-4">
      <div className="max-w-xl space-y-3">
        <h2 className="text-lg font-bold">General</h2>
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
        <Field label="Runs kept in history">
          <Input type="number" value={settings.historyLimit} onChange={(e) => set({ historyLimit: Number(e.target.value) })} />
        </Field>

        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={settings.maskInGrid} onChange={(e) => set({ maskInGrid: e.target.checked })} />
          Hide masked columns in results too, not only from the assistant
        </label>

        <h2 className="pt-3 text-lg font-bold">Assistant</h2>
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
          <Input type="password" autoComplete="off" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
        </Field>
        <ErrorNote message={error} />
        {message && <p className="text-sm">{message}</p>}
        <div className="flex gap-2">
          <Button tone="primary" onClick={() => void save()}>
            Save settings
          </Button>
          <Button onClick={() => void testProvider()}>Test the model</Button>
          <Button onClick={() => void copyDiagnostics()}>Copy diagnostics</Button>
          {diagnostics && <span className="text-sm">{diagnostics}</span>}
        </div>
        <MaskingEditor />
      </div>
    </div>
  );
}
