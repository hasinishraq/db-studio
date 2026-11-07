import { useState } from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import { open as openFile } from '@tauri-apps/plugin-dialog';
import { useQueryClient } from '@tanstack/react-query';
import { api, ApiError, errorMessage } from '../../api';
import type { ConnectionProfile, Engine, Environment, HostKeyDetail, ServerInfo, SshAuth, TlsMode } from '../../bindings';
import { Button, ErrorNote, Field, Input, Modal, Select } from '../ui';

const PORTS: Record<Engine, number> = { Postgres: 5432, MySql: 3306, Sqlite: 0 };

export function blankProfile(): ConnectionProfile {
  return {
    id: crypto.randomUUID(),
    name: '',
    engine: 'Postgres',
    host: 'localhost',
    port: 5432,
    database: '',
    username: '',
    tlsMode: 'VerifyFull',
    caCertPath: null,
    clientCertPath: null,
    clientKeyPath: null,
    ssh: null,
    environment: 'Development',
    readOnly: false,
    aiPolicy: 'Default',
    color: null,
    hasPassword: false,
  };
}

function FilePicker({ label, value, onChange }: { label: string; value: string | null; onChange: (v: string | null) => void }) {
  const pick = async () => {
    const chosen = await openFile({ multiple: false });
    if (typeof chosen === 'string') onChange(chosen);
  };
  return (
    <Field label={label}>
      <div className="flex gap-2">
        <Input value={value ?? ''} readOnly placeholder="None" />
        <Button onClick={() => void pick()}>Choose</Button>
        {value && <Button onClick={() => onChange(null)}>Clear</Button>}
      </div>
    </Field>
  );
}

/** A stored secret is never shown: the field offers only to replace it. */
function SecretField({
  label,
  saved,
  value,
  onChange,
}: {
  label: string;
  saved: boolean;
  value: string;
  onChange: (value: string) => void;
}) {
  const [replacing, setReplacing] = useState(false);
  return (
    <Field label={label}>
      {saved && !replacing ? (
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted">Saved</span>
          <Button onClick={() => setReplacing(true)}>Replace</Button>
        </div>
      ) : (
        <Input type="password" autoComplete="off" value={value} onChange={(e) => onChange(e.target.value)} />
      )}
    </Field>
  );
}

function ServerReport({ info }: { info: ServerInfo }) {
  const tls = !info.tls ? 'not encrypted' : info.tlsVerified ? 'encrypted and verified' : 'encrypted, not verified';
  return (
    <div className="rounded border-l-4 border-safe bg-ground p-2 text-sm">
      <p>{info.version}</p>
      <p className="text-muted">The connection is {tls}.</p>
      {(info.certificates ?? []).map((c) => (
        <dl key={c.fingerprintSha256} className="mt-2 font-mono text-xs">
          <dt className="text-muted">Subject</dt>
          <dd>{c.subject}</dd>
          <dt className="text-muted">Issuer</dt>
          <dd>{c.issuer}</dd>
          <dt className="text-muted">Valid</dt>
          <dd>
            {c.notBefore} to {c.notAfter}
          </dd>
          <dt className="text-muted">SHA-256</dt>
          <dd className="break-all">{c.fingerprintSha256}</dd>
        </dl>
      ))}
    </div>
  );
}

/** Shows a bastion's fingerprint so the user can compare it before trusting. */
export function HostKeyPrompt({
  detail,
  changed,
  onTrust,
  onCancel,
}: {
  detail: HostKeyDetail;
  changed: boolean;
  onTrust: () => void;
  onCancel: () => void;
}) {
  return (
    <div className={`rounded border-l-4 p-2 text-sm ${changed ? 'border-danger' : 'border-change'} bg-ground`}>
      <p className="font-bold">
        {changed ? `The host key of ${detail.host} changed` : `First connection to ${detail.host}`}
      </p>
      <p className="mb-1 text-muted">
        {changed
          ? 'This can mean someone is intercepting the connection. Replace the key only if you know why it changed.'
          : 'Compare this fingerprint with the one your administrator gave you.'}
      </p>
      {detail.expected && <p className="break-all font-mono text-xs">Expected {detail.expected}</p>}
      <p className="break-all font-mono text-xs">{detail.fingerprint}</p>
      <div className="mt-2 flex gap-2">
        <Button tone={changed ? 'danger' : 'change'} onClick={onTrust}>
          {changed ? 'Replace the key' : 'Trust this key'}
        </Button>
        <Button onClick={onCancel}>Cancel</Button>
      </div>
    </div>
  );
}

export function ConnectionDialog({ initial, onClose }: { initial: ConnectionProfile; onClose: () => void }) {
  const [profile, setProfile] = useState(initial);
  const [password, setPassword] = useState('');
  const [sshSecret, setSshSecret] = useState('');
  const [report, setReport] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hostKey, setHostKey] = useState<{ detail: HostKeyDetail; changed: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const client = useQueryClient();
  const set = (change: Partial<ConnectionProfile>) => setProfile((p) => ({ ...p, ...change }));
  const sqlite = profile.engine === 'Sqlite';

  const persistSecrets = async () => {
    if (sshSecret) await api.setSecret({ kind: 'sshSecret', profileId: profile.id }, sshSecret);
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.saveProfile(profile, password || null);
      await persistSecrets();
      await client.invalidateQueries({ queryKey: ['profiles'] });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const test = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      await persistSecrets();
      setReport(await api.testConnection(profile, password || null));
    } catch (e) {
      if (e instanceof ApiError && e.hostKey) {
        setHostKey({ detail: e.hostKey, changed: e.code === 'HostKeyChanged' });
      } else setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const trust = async () => {
    if (!hostKey) return;
    try {
      await api.saveProfile(profile, password || null);
      const { detail, changed } = hostKey;
      await api.trustHostKey(profile.id, detail.keyType ?? 'unknown', detail.fingerprint, changed);
      setHostKey(null);
      await test();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const ssh = profile.ssh;
  const setSsh = (change: Partial<NonNullable<ConnectionProfile['ssh']>>) =>
    set({ ssh: ssh ? { ...ssh, ...change } : null });

  return (
    <Modal title={initial.name ? `Edit ${initial.name}` : 'New connection'} onClose={onClose}>
      <Tabs.Root defaultValue="general">
        <Tabs.List className="mb-3 flex gap-1 border-b border-line">
          {['general', 'tls', 'ssh'].map((tab) => (
            <Tabs.Trigger
              key={tab}
              value={tab}
              className="px-3 py-1 text-sm data-[state=active]:border-b-2 data-[state=active]:border-ink"
            >
              {tab === 'general' ? 'General' : tab.toUpperCase()}
            </Tabs.Trigger>
          ))}
        </Tabs.List>

        <Tabs.Content value="general" className="space-y-2">
          <Field label="Name">
            <Input value={profile.name} onChange={(e) => set({ name: e.target.value })} />
          </Field>
          <Field label="Engine">
            <Select
              value={profile.engine}
              onChange={(e) => {
                const engine = e.target.value as Engine;
                set({ engine, port: PORTS[engine], tlsMode: engine === 'Sqlite' ? 'Disable' : profile.tlsMode });
              }}
            >
              <option value="Postgres">PostgreSQL</option>
              <option value="MySql">MySQL or MariaDB</option>
              <option value="Sqlite">SQLite</option>
            </Select>
          </Field>
          {sqlite ? (
            <FilePicker label="Database file" value={profile.database || null} onChange={(v) => set({ database: v ?? '' })} />
          ) : (
            <>
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2">
                  <Field label="Host">
                    <Input value={profile.host} onChange={(e) => set({ host: e.target.value })} />
                  </Field>
                </div>
                <Field label="Port">
                  <Input
                    type="number"
                    value={profile.port}
                    onChange={(e) => set({ port: Number(e.target.value) })}
                  />
                </Field>
              </div>
              <Field label="Database">
                <Input
                  placeholder="Optional. Choose one after connecting"
                  value={profile.database}
                  onChange={(e) => set({ database: e.target.value })}
                />
              </Field>
              <Field label="User">
                <Input value={profile.username} onChange={(e) => set({ username: e.target.value })} />
              </Field>
              <SecretField label="Password" saved={profile.hasPassword ?? false} value={password} onChange={setPassword} />
            </>
          )}
          <Field label="Environment">
            <Select
              value={profile.environment}
              onChange={(e) => set({ environment: e.target.value as Environment })}
            >
              <option value="Development">Development</option>
              <option value="Staging">Staging</option>
              <option value="Production">Production</option>
            </Select>
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={profile.readOnly} onChange={(e) => set({ readOnly: e.target.checked })} />
            Read-only: refuse every change, even if approved
          </label>
        </Tabs.Content>

        <Tabs.Content value="tls" className="space-y-2">
          <Field label="Mode">
            <Select value={profile.tlsMode} onChange={(e) => set({ tlsMode: e.target.value as TlsMode })}>
              <option value="VerifyFull">Verify full</option>
              <option value="VerifyCa">Verify CA</option>
              <option value="Require">Require (encrypt, do not verify)</option>
              <option value="Disable">Disable</option>
            </Select>
          </Field>
          {profile.tlsMode === 'Require' && (
            <p className="border-l-4 border-change pl-2 text-sm">
              Traffic is encrypted, but the server's identity is not checked.
            </p>
          )}
          {profile.tlsMode === 'Disable' && (
            <p className="border-l-4 border-danger pl-2 text-sm">
              Passwords and data travel unencrypted. Use this only for a local server.
            </p>
          )}
          <FilePicker label="CA certificate" value={profile.caCertPath} onChange={(v) => set({ caCertPath: v })} />
          <FilePicker label="Client certificate" value={profile.clientCertPath} onChange={(v) => set({ clientCertPath: v })} />
          <FilePicker label="Client key" value={profile.clientKeyPath} onChange={(v) => set({ clientKeyPath: v })} />
        </Tabs.Content>

        <Tabs.Content value="ssh" className="space-y-2">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={ssh !== null}
              onChange={(e) =>
                set({
                  ssh: e.target.checked
                    ? { host: '', port: 22, username: '', auth: { kind: 'password' }, hasSecret: false }
                    : null,
                })
              }
            />
            Connect through an SSH bastion
          </label>
          {ssh && (
            <>
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2">
                  <Field label="Bastion host">
                    <Input value={ssh.host} onChange={(e) => setSsh({ host: e.target.value })} />
                  </Field>
                </div>
                <Field label="Port">
                  <Input type="number" value={ssh.port} onChange={(e) => setSsh({ port: Number(e.target.value) })} />
                </Field>
              </div>
              <Field label="User">
                <Input value={ssh.username} onChange={(e) => setSsh({ username: e.target.value })} />
              </Field>
              <Field label="Sign in with">
                <Select
                  value={ssh.auth.kind}
                  onChange={(e) => {
                    const kind = e.target.value as SshAuth['kind'];
                    setSsh({ auth: kind === 'privateKey' ? { kind, path: '' } : { kind } });
                  }}
                >
                  <option value="password">Password</option>
                  <option value="privateKey">Private key</option>
                  <option value="agent">SSH agent</option>
                </Select>
              </Field>
              {ssh.auth.kind === 'privateKey' && (
                <FilePicker
                  label="Key file"
                  value={ssh.auth.path || null}
                  onChange={(v) => setSsh({ auth: { kind: 'privateKey', path: v ?? '' } })}
                />
              )}
              {ssh.auth.kind !== 'agent' && (
                <SecretField
                  label={ssh.auth.kind === 'password' ? 'SSH password' : 'Key passphrase'}
                  saved={ssh.hasSecret ?? false}
                  value={sshSecret}
                  onChange={setSshSecret}
                />
              )}
            </>
          )}
        </Tabs.Content>
      </Tabs.Root>

      <div className="mt-3 space-y-2">
        <ErrorNote message={error} />
        {report && <ServerReport info={report} />}
        {hostKey && (
          <HostKeyPrompt {...hostKey} onTrust={() => void trust()} onCancel={() => setHostKey(null)} />
        )}
      </div>
      <div className="mt-4 flex justify-between">
        <Button disabled={busy} onClick={() => void test()}>
          Test connection
        </Button>
        <div className="flex gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="primary" disabled={busy} onClick={() => void save()}>
            Save
          </Button>
        </div>
      </div>
    </Modal>
  );
}
