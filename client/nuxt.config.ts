// The Nuxt project the mail screen is typed and prepared in (docs/api.md, "The console plugin bundle"): the
// screen's page, components, domain modules and language pack on the bmo-ui layer, standing as a console does,
// one unprefixed locale. `nuxi prepare` writes the declarations the bundle build reads which names Nuxt imports
// and registers by itself.
export default defineNuxtConfig({
  extends: ["bmo-ui"],
  modules: ["@nuxtjs/i18n"],
  // The screen's own words, one JSON file to a locale under i18n/locales/, which the bundle build gathers into
  // the bundle's words.
  i18n: {
    strategy: "no_prefix",
    defaultLocale: "ko",
    locales: [{ code: "ko", language: "ko-KR", file: "ko.json" }]
  },
  compatibilityDate: "2025-09-01",
  devtools: { enabled: false }
});
