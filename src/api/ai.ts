import { Channel } from '@tauri-apps/api/core';
import { commands } from '../bindings';
import type { AiEvent, AiSendRequest } from '../bindings';
import { unwrap } from './errors';

/** Starts a turn on its own channel, so events need no global filtering. */
export function sendTurn(request: AiSendRequest, onEvent: (event: AiEvent) => void): Promise<null> {
  const channel = new Channel<AiEvent>();
  channel.onmessage = onEvent;
  return unwrap(commands.aiSend(request, channel));
}

export const reviewQuery = (turn: string, query: string, approved: boolean) =>
  unwrap(commands.aiReview(turn, query, approved));
export const cancelTurn = (turn: string) => unwrap(commands.aiCancel(turn));
export const cancelQuery = (turn: string, query: string) => unwrap(commands.aiCancelQuery(turn, query));
