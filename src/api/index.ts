import { commands } from '../bindings';
import type {
  AuditQuery,
  ConnectionProfile,
  ConversationId,
  Draft,
  ExportFormat,
  FilterRule,
  ImportMapping,
  MaskingRule,
  NewColumn,
  Origin,
  PageRequest,
  ProfileId,
  RowChange,
  SchemaRef,
  SecretTarget,
  SessionId,
  Settings,
  TableRef,
  TxMode,
} from '../bindings';
import { Channel } from '@tauri-apps/api/core';
import { unwrap } from './errors';

export { ApiError, errorMessage, unwrap } from './errors';

/** The only route from components to Rust. */
export const api = {
  getProfiles: () => unwrap(commands.listProfiles()),
  saveProfile: (profile: ConnectionProfile, password: string | null) =>
    unwrap(commands.saveProfile(profile, password)),
  deleteProfile: (id: ProfileId) => unwrap(commands.deleteProfile(id)),
  setSecret: (target: SecretTarget, value: string) => unwrap(commands.setSecret(target, value)),
  clearSecret: (target: SecretTarget) => unwrap(commands.clearSecret(target)),
  testConnection: (profile: ConnectionProfile, password: string | null) =>
    unwrap(commands.testConnection(profile, password)),
  connect: (id: ProfileId) => unwrap(commands.connect(id)),
  disconnect: (session: SessionId, force: boolean) => unwrap(commands.disconnect(session, force)),
  trustHostKey: (id: ProfileId, keyType: string, fingerprint: string, replace: boolean) =>
    unwrap(commands.trustHostKey(id, keyType, fingerprint, replace)),

  listSchemas: (session: SessionId) => unwrap(commands.listSchemas(session)),
  listObjects: (session: SessionId, schema: SchemaRef) => unwrap(commands.listObjects(session, schema)),
  columns: (session: SessionId, schema: SchemaRef) => unwrap(commands.columns(session, schema)),
  tableStructure: (session: SessionId, table: TableRef) => unwrap(commands.tableStructure(session, table)),
  tablePage: (session: SessionId, table: TableRef, page: PageRequest) =>
    unwrap(commands.tablePage(session, table, page)),
  tableCount: (session: SessionId, table: TableRef, filters: FilterRule[]) =>
    unwrap(commands.tableCount(session, table, filters)),

  prepareQuery: (session: SessionId, sql: string) => unwrap(commands.prepareQuery(session, sql)),
  approveQuery: (session: SessionId, sql: string, origin: Origin) =>
    unwrap(commands.approveQuery(session, sql, origin)),
  cancelRun: (run: string) => unwrap(commands.cancelRun(run)),
  explain: (session: SessionId, sql: string) => unwrap(commands.explainQuery(session, sql)),
  setTxMode: (session: SessionId, mode: TxMode) => unwrap(commands.setTxMode(session, mode)),
  commitTx: (session: SessionId) => unwrap(commands.commitTx(session)),
  rollbackTx: (session: SessionId) => unwrap(commands.rollbackTx(session)),
  txState: (session: SessionId) => unwrap(commands.txState(session)),
  planRowChange: (session: SessionId, table: TableRef, change: RowChange) =>
    unwrap(commands.planRowChange(session, table, change)),
  planCreateTable: (session: SessionId, table: TableRef, columns: NewColumn[]) =>
    unwrap(commands.planCreateTable(session, table, columns)),

  historyList: (limit: number) => unwrap(commands.historyList(limit)),
  historyClear: () => unwrap(commands.historyClear()),
  draftsList: () => unwrap(commands.draftsList()),
  draftsSave: (draft: Draft) => unwrap(commands.draftsSave(draft)),
  draftsDelete: (id: string) => unwrap(commands.draftsDelete(id)),
  settingsGet: () => unwrap(commands.settingsGet()),
  settingsSet: (settings: Settings) => unwrap(commands.settingsSet(settings)),

  exportQuery: (session: SessionId, sql: string, format: ExportFormat, run: string, onRows: (rows: number) => void) => {
    const channel = new Channel<number>();
    channel.onmessage = onRows;
    return unwrap(commands.exportQuery(session, sql, format, run, channel));
  },
  exportTable: (session: SessionId, table: TableRef, format: ExportFormat, run: string, onRows: (rows: number) => void) => {
    const channel = new Channel<number>();
    channel.onmessage = onRows;
    return unwrap(commands.exportTable(session, table, format, run, channel));
  },
  importPickFile: () => unwrap(commands.importPickFile()),
  importPlan: (session: SessionId, file: string, table: TableRef, mappings: ImportMapping[]) =>
    unwrap(commands.importPlan(session, file, table, mappings)),
  importCsv: (
    session: SessionId,
    file: string,
    table: TableRef,
    mappings: ImportMapping[],
    batch: number,
    token: string,
    run: string,
  ) => unwrap(commands.importCsv(session, file, table, mappings, batch, token, run)),

  auditList: (query: AuditQuery) => unwrap(commands.auditList(query)),
  auditVerify: () => unwrap(commands.auditVerify()),
  auditExport: (query: AuditQuery, format: ExportFormat) => unwrap(commands.auditExport(query, format)),

  aiConversations: () => unwrap(commands.aiConversations()),
  aiTurns: (id: ConversationId) => unwrap(commands.aiTurns(id)),
  aiRename: (id: ConversationId, title: string) => unwrap(commands.aiRename(id, title)),
  aiForget: (id: ConversationId, from: string | null) => unwrap(commands.aiForget(id, from)),
  aiTestProvider: () => unwrap(commands.aiTestProvider()),
  maskingList: (id: ProfileId) => unwrap(commands.maskingList(id)),
  maskingSet: (id: ProfileId, rules: MaskingRule[]) => unwrap(commands.maskingSet(id, rules)),

  diagnostics: () => unwrap(commands.diagnostics()),
  importLegacy: () => unwrap(commands.importLegacy()),
};
