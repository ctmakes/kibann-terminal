// The default screen (screens/default/screen.json), shown from the start
// until the app says which screen the config names. A test keeps the two
// the same.

import type { ScreenNow } from "../proto";

export const DEFAULT_SCREEN: ScreenNow = {
  name: "default",
  problem: "",
  parts: {},
  screen: {
    about: "Kibann's window: the sidebar, the tabs and the terminals",
    chrome: { colors: {} },
    floating: [],
    root: {
      type: "row",
      resizable: false,
      hidden: false,
      style: {},
      children: [
        { type: "block", size: "auto", content: { part: "sidebar" }, resizable: false, hidden: false, children: [], style: {} },
        {
          type: "column",
          resizable: false,
          hidden: false,
          style: {},
          children: [
            { type: "block", size: "auto", content: { part: "tabs" }, resizable: false, hidden: false, children: [], style: {} },
            { type: "block", content: { tab: "active" }, resizable: false, hidden: false, children: [], style: {} },
          ],
        },
      ],
    },
  },
};
