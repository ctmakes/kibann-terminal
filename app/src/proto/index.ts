// Re-exports of the generated protocol types and constants.

export type { Frame } from "./generated/Frame";
export type { Style } from "./generated/Style";
export type { Row } from "./generated/Row";
export type { Run } from "./generated/Run";
export type { Cursor } from "./generated/Cursor";
export type { CursorStyle } from "./generated/CursorStyle";
export type { MouseTracking } from "./generated/MouseTracking";
export type { Resize } from "./generated/Resize";
export type { InputEvent } from "./generated/InputEvent";
export type { KeyAction } from "./generated/KeyAction";
export type { MouseAction } from "./generated/MouseAction";
export type { Scrollbar } from "./generated/Scrollbar";
export type { ScrollBy } from "./generated/ScrollBy";
export type { Link } from "./generated/Link";
export type { LinkSpan } from "./generated/LinkSpan";
export type { Find } from "./generated/Find";
export type { FindSpan } from "./generated/FindSpan";
export type { Layout } from "./generated/Layout";
export type { LayoutTab } from "./generated/LayoutTab";
export type { LayoutAction } from "./generated/LayoutAction";
export type { Template } from "./generated/Template";
export type { TabTemplate } from "./generated/TabTemplate";
export type { TemplateTree } from "./generated/TemplateTree";
export type { Tree } from "./generated/Tree";
export type { Dir } from "./generated/Dir";
export type { Step } from "./generated/Step";
export type { Command } from "./generated/Command";
export type { Direction } from "./generated/Direction";
export type { TemplateRun } from "./generated/TemplateRun";
export type { AgentState } from "./generated/AgentState";
export type { PaneStatus } from "./generated/PaneStatus";
export type { GitInfo } from "./generated/GitInfo";
export type { GitOp } from "./generated/GitOp";
export type { PaneDir } from "./generated/PaneDir";
export type { Theme } from "./generated/Theme";
export type { Settings } from "./generated/Settings";
export type { Screen } from "./generated/Screen";
export type { ScreenNode } from "./generated/ScreenNode";
export type { ScreenContent } from "./generated/ScreenContent";
export type { ScreenFloating } from "./generated/ScreenFloating";
export type { ScreenSize } from "./generated/ScreenSize";
export type { ScreenEntry } from "./generated/ScreenEntry";
export type { ScreenNow } from "./generated/ScreenNow";
export type { PartNow } from "./generated/PartNow";
export type { PartManifest } from "./generated/PartManifest";
export type { Permissions } from "./generated/Permissions";
export type { PartState } from "./generated/PartState";
export type { PartTheme } from "./generated/PartTheme";
export type { Acted } from "./generated/Acted";
export type { ConfigInfo } from "./generated/ConfigInfo";
export type { ThemeEntry } from "./generated/ThemeEntry";
export type { ThemeNow } from "./generated/ThemeNow";
export type { UpdateReady } from "./generated/UpdateReady";
export type { UpdateStatus } from "./generated/UpdateStatus";
export type { WorkspaceEntry } from "./generated/WorkspaceEntry";

export { Flag, Mods, Underline } from "./generated/constants";
import { Flag } from "./generated/constants";

export function underlineOf(flags: number): number {
  return (flags & Flag.UNDERLINE_MASK) >> Flag.UNDERLINE_SHIFT;
}

export function withUnderline(flags: number, ul: number): number {
  return (flags & ~Flag.UNDERLINE_MASK) | (ul << Flag.UNDERLINE_SHIFT);
}
