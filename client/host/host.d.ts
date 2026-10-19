// What the host owes the screen, declared for the bundle's own typecheck alone (docs/api.md, "The console
// plugin bundle"). A host registers the three components by name and makes the function importable by name,
// as Nuxt makes a project's own composables; nothing here ships.

import type { DefineComponent } from "vue";

declare module "vue" {
  export interface GlobalComponents {
    // The three pieces of the console's title bar, which take no props from the screen.
    DeployMark: DefineComponent<Record<string, never>>;
    TodoOpenButton: DefineComponent<Record<string, never>>;
    UsageMeters: DefineComponent<Record<string, never>>;
  }
}

declare global {
  // The mail word of the tab's one event stream: called with the account key of each change the console
  // relays from the service's event stream, and with null each time the tab's stream opens.
  function useMailEvents(onChange: (account: string | null) => void): void;
}

export {};
