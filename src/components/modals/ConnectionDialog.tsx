import { useState, type ReactNode } from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import { open as openFile } from '@tauri-apps/plugin-dialog';
import { useQueryClient } from '@tanstack/react-query';
import { CircleCheck, FolderOpen, KeyRound, LoaderCircle, Lock, LockOpen, X } from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import type { ConnectionProfile, Engine, Environment, HostKeyDetail, ServerInfo, SshAuth, TlsMode } from '../../bindings';
import { Button, Callout, cn, ErrorNote, Input, Modal, Segmented, Select } from '../ui';

const PORTS: Record<Engine, number> = { Postgres: 5432, MySql: 3306, Sqlite: 0 };

const ENGINES: { value: Engine; label: string; hint: string }[] = [
  { value: 'Postgres', label: 'PostgreSQL', hint: 'Port 5432' },
  { value: 'MySql', label: 'MySQL or MariaDB', hint: 'Port 3306' },
  { value: 'Sqlite', label: 'SQLite', hint: 'A local file' },
];

const ENVIRONMENTS: { value: Environment; dot: string }[] = [
  { value: 'Development', dot: 'bg-safe' },
  { value: 'Staging', dot: 'bg-change' },
  { value: 'Production', dot: 'bg-danger' },
];

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

/** The address a profile points at, for a reader to check. It never contains a secret. */
export function connectionUrl(profile: ConnectionProfile): string {
  if (profile.engine === 'Sqlite') return profile.database ? `sqlite:${profile.database}` : 'sqlite:';
  const scheme = profile.engine === 'Postgres' ? 'postgresql' : 'mysql';
  const user = profile.username ? `${encodeURIComponent(profile.username)}@` : '';
  const database = profile.database ? `/${encodeURIComponent(profile.database)}` : '';
  return `${scheme}://${user}${profile.host || 'localhost'}:${profile.port}${database}`;
}

/** A form row: the label on the left, the control on the right, as in desktop settings panes. */
function Row({ label, children, hint }: { label: string; children: ReactNode; hint?: ReactNode }) {
  return (
    <label className="grid grid-cols-[120px_minmax(0,1fr)] items-center gap-x-4 gap-y-1 text-sm">
      <span className="text-right text-muted">{label}</span>
      <span className="min-w-0">{children}</span>
      {hint && <span className="col-start-2 text-xs text-muted">{hint}</span>}
    </label>
  );
}

/** A row whose right side holds several controls or text, so it cannot be one label. */
function Stacked({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[120px_minmax(0,1fr)] items-start gap-x-4 text-sm">
      <span className="pt-1 text-right text-muted">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function FilePicker({ label, value, onChange }: { label: string; value: string | null; onChange: (v: string | null) => void }) {
  const pick = async () => {
    const chosen = await openFile({ multiple: false });
    if (typeof chosen === 'string') onChange(chosen);
  };
  return (
    <Stacked label={label}>
      <div className="flex gap-1.5">
        <Input aria-label={label} className="font-mono text-xs" value={value ?? ''} readOnly placeholder="None" title={value ?? undefined} />
        <Button onClick={() => void pick()} title="Choose a file">
          <FolderOpen size={14} aria-hidden /> Choose
        </Button>
        {value && (
          <Button tone="quiet" className="w-[26px] px-0" aria-label={`Clear ${label}`} title="Clear" onClick={() => onChange(null)}>
            <X size={14} aria-hidden />
          </Button>
        )}
      </div>
    </Stacked>
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
  if (saved && !replacing) {
    return (
      <Stacked label={label}>
        <div className="flex h-[26px] items-center gap-2">
          <span className="flex items-center gap-1.5 text-sm">
            <KeyRound size={13} aria-hidden className="text-muted" />
            <span>Saved</span>
          </span>
          <span className="text-xs text-muted">in the system keychain</span>
          <Button tone="quiet" className="ml-auto h-6 px-2 text-xs" onClick={() => setReplacing(true)}>
            Replace
          </Button>
        </div>
      </Stacked>
    );
  }
  return (
    <Row label={label}>
      <Input type="password" autoComplete="off" value={value} onChange={(e) => onChange(e.target.value)} />
    </Row>
  );
}

function ServerReport({ info }: { info: ServerInfo }) {
  const verified = info.tls && info.tlsVerified;
  const TlsIcon = info.tls ? Lock : LockOpen;
  return (
    <div role="status" className="rounded-md border border-safe/35 bg-safe/10 px-3 py-2 text-sm">
      <p className="flex items-center gap-2 font-medium">
        <CircleCheck size={15} aria-hidden className="shrink-0 text-safe" />
        Connected
        <span className="truncate font-normal text-muted">{info.version}</span>
      </p>
      <p className={cn('mt-1 flex items-center gap-1.5 pl-[23px] text-xs', verified ? 'text-muted' : 'text-change')}>
        <TlsIcon size={12} aria-hidden />
        {!info.tls ? 'Not encrypted' : info.tlsVerified ? 'Encrypted, certificate verified' : 'Encrypted, certificate not verified'}
      </p>
      {(info.certificates ?? []).length > 0 && (
        <details className="mt-2 pl-[23px]">
          <summary className="cursor-pointer text-xs text-muted hover:text-ink">Certificate chain</summary>
          {(info.certificates ?? []).map((c) => (
            <dl
              key={c.fingerprintSha256}
              className="mt-2 grid grid-cols-[80px_minmax(0,1fr)] gap-x-3 gap-y-0.5 border-t border-line pt-2 font-mono text-xs"
            >
              <dt className="text-muted">Subject</dt>
              <dd className="break-all">{c.subject}</dd>
              <dt className="text-muted">Issuer</dt>
              <dd className="break-all">{c.issuer}</dd>
              <dt className="text-muted">Valid</dt>
              <dd>
                {c.notBefore} to {c.notAfter}
              </dd>
              <dt className="text-muted">SHA-256</dt>
              <dd className="break-all">{c.fingerprintSha256}</dd>
            </dl>
          ))}
        </details>
      )}
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
    <Callout
      tone={changed ? 'danger' : 'change'}
      title={changed ? `The host key of ${detail.host} changed` : `First connection to ${detail.host}`}
    >
      <p className="mb-2 text-muted">
        {changed
          ? 'This can mean someone is intercepting the connection. Replace the key only if you know why it changed.'
          : 'Compare this fingerprint with the one your administrator gave you.'}
      </p>
      <dl className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-3 gap-y-0.5 font-mono text-xs">
        {detail.expected && (
          <>
            <dt className="text-muted">Expected</dt>
            <dd className="break-all">{detail.expected}</dd>
          </>
        )}
        <dt className="text-muted">{detail.expected ? 'Offered' : 'Key'}</dt>
        <dd className="break-all">{detail.fingerprint}</dd>
      </dl>
      <div className="mt-3 flex gap-2">
        <Button tone={changed ? 'danger' : 'change'} onClick={onTrust}>
          {changed ? 'Replace the key' : 'Trust this key'}
        </Button>
        <Button onClick={onCancel}>Cancel</Button>
      </div>
    </Callout>
  );
}

const TAB =
  'relative h-9 px-3 text-sm text-muted hover:text-ink data-[state=active]:text-ink data-[state=active]:after:absolute data-[state=active]:after:inset-x-2 data-[state=active]:after:bottom-0 data-[state=active]:after:h-0.5 data-[state=active]:after:rounded-full data-[state=active]:after:bg-accent';

export function ConnectionDialog({ initial, onClose }: { initial: ConnectionProfile; onClose: () => void }) {
  const [profile, setProfile] = useState(initial);
  const [password, setPassword] = useState('');
  const [sshSecret, setSshSecret] = useState('');
  const [report, setReport] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hostKey, setHostKey] = useState<{ detail: HostKeyDetail; changed: boolean } | null>(null);
  const [busy, setBusy] = useState<'test' | 'save' | null>(null);
  const [tab, setTab] = useState('general');
  const client = useQueryClient();
  const set = (change: Partial<ConnectionProfile>) => setProfile((p) => ({ ...p, ...change }));
  const sqlite = profile.engine === 'Sqlite';

  const persistSecrets = async () => {
    if (sshSecret) await api.setSecret({ kind: 'sshSecret', profileId: profile.id }, sshSecret);
  };

  const save = async () => {
    setBusy('save');
    setError(null);
    try {
      await api.saveProfile(profile, password || null);
      await persistSecrets();
      await client.invalidateQueries({ queryKey: ['profiles'] });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const test = async () => {
    setBusy('test');
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
      setBusy(null);
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

  const footer = (
    <>
      <Button disabled={busy !== null} onClick={() => void test()}>
        {busy === 'test' && <LoaderCircle size={14} aria-hidden className="animate-spin" />}
        Test connection
      </Button>
      <div className="ml-auto flex gap-2">
        <Button onClick={onClose}>Cancel</Button>
        <Button tone="primary" disabled={busy !== null} onClick={() => void save()}>
          Save
        </Button>
      </div>
    </>
  );

  return (
    <Modal title={initial.name ? `Edit ${initial.name}` : 'New connection'} onClose={onClose} footer={footer} className="max-w-2xl">
      <div className="-mx-5 -mt-5 mb-4 border-b border-line bg-ground px-5 py-4">
        <div role="radiogroup" aria-label="Engine" className="grid grid-cols-3 gap-2">
          {ENGINES.map((engine) => {
            const chosen = profile.engine === engine.value;
            return (
              <label
                key={engine.value}
                className={cn(
                  'flex cursor-pointer flex-col rounded-md border px-3 py-2 transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-accent',
                  chosen ? 'border-accent bg-accent/10' : 'border-line bg-panel hover:border-edge',
                )}
              >
                <input
                  type="radio"
                  name="engine"
                  className="sr-only"
                  value={engine.value}
                  checked={chosen}
                  onChange={() =>
                    set({
                      engine: engine.value,
                      port: PORTS[engine.value],
                      tlsMode: engine.value === 'Sqlite' ? 'Disable' : profile.tlsMode,
                    })
                  }
                />
                <span className="text-sm font-medium">{engine.label}</span>
                <span className="text-xs text-muted">{engine.hint}</span>
              </label>
            );
          })}
        </div>
      </div>

      <div className="space-y-2.5">
        <Row label="Name">
          <Input autoFocus placeholder="Orders, staging" value={profile.name} onChange={(e) => set({ name: e.target.value })} />
        </Row>
        <Stacked label="Environment">
          <Segmented
            label="Environment"
            value={profile.environment}
            onChange={(environment) => set({ environment })}
            options={ENVIRONMENTS.map((env) => ({
              value: env.value,
              label: (
                <>
                  <span aria-hidden className={cn('h-1.5 w-1.5 rounded-full', env.dot)} />
                  {env.value}
                </>
              ),
            }))}
          />
        </Stacked>
      </div>

      {/* SQLite has no TLS or SSH, so its tabs fall back to Connection. */}
      <Tabs.Root value={sqlite ? 'general' : tab} onValueChange={setTab} className="mt-4">
        <Tabs.List className="-mx-5 mb-4 flex border-b border-line px-3">
          <Tabs.Trigger value="general" className={TAB}>
            Connection
          </Tabs.Trigger>
          {!sqlite && (
            <Tabs.Trigger value="tls" className={TAB}>
              TLS
            </Tabs.Trigger>
          )}
          {!sqlite && (
            <Tabs.Trigger value="ssh" className={TAB}>
              <span className="flex items-center gap-1.5">
                SSH tunnel
                {ssh && <span aria-label="on" className="h-1.5 w-1.5 rounded-full bg-accent" />}
              </span>
            </Tabs.Trigger>
          )}
        </Tabs.List>

        <Tabs.Content value="general" className="space-y-2.5 focus:outline-none">
          {sqlite ? (
            <FilePicker label="Database file" value={profile.database || null} onChange={(v) => set({ database: v ?? '' })} />
          ) : (
            <>
              <Stacked label="Host">
                <div className="flex items-center gap-1.5">
                  <Input aria-label="Host" value={profile.host} onChange={(e) => set({ host: e.target.value })} />
                  <span aria-hidden className="text-muted">
                    :
                  </span>
                  <Input
                    aria-label="Port"
                    className="w-20 tabular-nums"
                    type="number"
                    value={profile.port}
                    onChange={(e) => set({ port: Number(e.target.value) })}
                  />
                </div>
              </Stacked>
              <Row label="User">
                <Input value={profile.username} onChange={(e) => set({ username: e.target.value })} />
              </Row>
              <SecretField label="Password" saved={profile.hasPassword ?? false} value={password} onChange={setPassword} />
              <Row label="Database">
                <Input
                  placeholder="Optional. Choose one after connecting"
                  value={profile.database}
                  onChange={(e) => set({ database: e.target.value })}
                />
              </Row>
            </>
          )}
          <Stacked label="Safety">
            <label className="flex h-[26px] items-center gap-2 text-sm">
              <input type="checkbox" checked={profile.readOnly} onChange={(e) => set({ readOnly: e.target.checked })} />
              Read-only: refuse every change, even if approved
            </label>
          </Stacked>
          <Stacked label="URL">
            <code className="block truncate rounded border border-line bg-ground px-2 py-1 font-mono text-xs text-muted">
              {connectionUrl(profile)}
            </code>
          </Stacked>
        </Tabs.Content>

        <Tabs.Content value="tls" className="space-y-2.5 focus:outline-none">
          <Row label="Mode">
            <Select value={profile.tlsMode} onChange={(e) => set({ tlsMode: e.target.value as TlsMode })}>
              <option value="VerifyFull">Verify full</option>
              <option value="VerifyCa">Verify CA</option>
              <option value="Require">Require (encrypt, do not verify)</option>
              <option value="Disable">Disable</option>
            </Select>
          </Row>
          {profile.tlsMode === 'Require' && (
            <div className="pl-[136px]">
              <Callout tone="change">Traffic is encrypted, but the server's identity is not checked.</Callout>
            </div>
          )}
          {profile.tlsMode === 'Disable' && (
            <div className="pl-[136px]">
              <Callout tone="danger">Passwords and data travel unencrypted. Use this only for a local server.</Callout>
            </div>
          )}
          <FilePicker label="CA certificate" value={profile.caCertPath} onChange={(v) => set({ caCertPath: v })} />
          <FilePicker label="Client certificate" value={profile.clientCertPath} onChange={(v) => set({ clientCertPath: v })} />
          <FilePicker label="Client key" value={profile.clientKeyPath} onChange={(v) => set({ clientKeyPath: v })} />
        </Tabs.Content>

        <Tabs.Content value="ssh" className="space-y-2.5 focus:outline-none">
          <Stacked label="Tunnel">
            <label className="flex h-[26px] items-center gap-2 text-sm">
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
          </Stacked>
          {ssh && (
            <>
              <Stacked label="Bastion host">
                <div className="flex items-center gap-1.5">
                  <Input aria-label="Bastion host" value={ssh.host} onChange={(e) => setSsh({ host: e.target.value })} />
                  <span aria-hidden className="text-muted">
                    :
                  </span>
                  <Input
                    aria-label="SSH port"
                    className="w-20 tabular-nums"
                    type="number"
                    value={ssh.port}
                    onChange={(e) => setSsh({ port: Number(e.target.value) })}
                  />
                </div>
              </Stacked>
              <Row label="User">
                <Input value={ssh.username} onChange={(e) => setSsh({ username: e.target.value })} />
              </Row>
              <Row label="Sign in with">
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
              </Row>
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

      {(error || report || hostKey) && (
        <div className="mt-4 space-y-2">
          <ErrorNote message={error} />
          {report && <ServerReport info={report} />}
          {hostKey && <HostKeyPrompt {...hostKey} onTrust={() => void trust()} onCancel={() => setHostKey(null)} />}
        </div>
      )}
    </Modal>
  );
}
