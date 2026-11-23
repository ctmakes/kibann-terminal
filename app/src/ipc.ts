// Commands to the app and events from it, by the names the app's Rust
// generates (`proto/generated/tauri.ts`): a misspelt or removed one fails
// to compile.

import { invoke as call, type InvokeArgs } from "@tauri-apps/api/core";
import {
  listen as on,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";
import type { Command, EventName } from "./proto/generated/tauri";

export function invoke<T = void>(cmd: Command, args?: InvokeArgs): Promise<T> {
  return call<T>(cmd, args);
}

export function listen<T = void>(
  event: EventName,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  return on<T>(event, handler);
}
