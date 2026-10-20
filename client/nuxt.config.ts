import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import type { Nuxt } from "@nuxt/schema";

// The mail screen as a console bundle (docs/api.md, "The console plugin bundle"): a layer on the bmo-ui
// layer adding the screen's page, components, domain modules and language pack. A console attaches it by
// its declaration, `console-bundle.json`, and relays the mail service to it under its relay base.
const here = dirname(fileURLToPath(import.meta.url));

export default defineNuxtConfig({
  extends: ["bmo-ui"],
  modules: [
    // Where the bundle is the project being prepared it stands as a console does, one unprefixed locale by
    // default, ahead of the i18n module that reads it; a console extending the bundle keeps its own choice.
    (_options: unknown, nuxt: Nuxt) => {
      if (nuxt.options.rootDir === here) {
        nuxt.options.i18n = { ...nuxt.options.i18n, strategy: "no_prefix", defaultLocale: "ko" };
      }
    },
    "@nuxtjs/i18n"
  ],
  // The screen's own words, one JSON file to a locale under i18n/locales/, the folder the declaration names;
  // a console's i18n module reads them as part of its locale of the same code.
  i18n: {
    locales: [{ code: "ko", language: "ko-KR", file: "ko.json" }]
  },
  compatibilityDate: "2025-09-01",
  devtools: { enabled: false }
});
