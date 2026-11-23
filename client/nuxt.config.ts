// The Nuxt project the mail screen is typed and prepared in (docs/api.md, "The console plugin bundle"): the
// screen's page, components, domain modules and language pack on the bmo-ui layer, standing as a console does,
// four unprefixed locales, Korean, English, Japanese and Simplified Chinese, of which the console names the one
// shown. `nuxi prepare` writes the declarations the bundle build reads which names Nuxt imports and registers by
// itself.
export default defineNuxtConfig({
  extends: ["bmo-ui"],
  modules: ["@nuxtjs/i18n"],
  // The screen's own words, one JSON file to a locale under i18n/locales/, which the bundle build gathers into
  // the bundle's words.
  i18n: {
    strategy: "no_prefix",
    defaultLocale: "ko",
    // The console names the locale a screen shows; the project itself reads no browser's language, so the
    // screen stands in the default one wherever it is typed and tested.
    detectBrowserLanguage: false,
    locales: [
      { code: "ko", language: "ko-KR", file: "ko.json" },
      { code: "en", language: "en-US", file: "en.json" },
      { code: "ja", language: "ja-JP", file: "ja.json" },
      { code: "zh-CN", language: "zh-CN", file: "zh-CN.json" }
    ]
  },
  compatibilityDate: "2025-09-01",
  devtools: { enabled: false }
});
