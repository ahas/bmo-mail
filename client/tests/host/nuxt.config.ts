// The host a browser test stands the mail screen in: the client's Nuxt project as a layer, under the one
// app component a console wraps its screens in, so the screen meets the providers it meets in a console.
// The console's own words for the layer's keys are the console's and stand outside this repository, so a
// test finds what it drives by role, address, the screen's data attributes and the screen's own words read
// from its language pack by key.
export default defineNuxtConfig({
  extends: ["../.."],
  devtools: { enabled: false },
  // A console renders a bundle's screen in the browser alone, and the server rendering a screen whose calls
  // reach the same server under `/mail` asks itself again with every call, until it stops answering.
  ssr: false,
  // The packages the screen loads once it stands, bundled before the first page, so the dev server does not
  // bundle them again under a page already loading and send it round again.
  vite: {
    optimizeDeps: {
      include: ["@tiptap/extension-table", "@tiptap/extension-text-align", "@tiptap/extension-text-style", "bmo-text/web-address", "dompurify"]
    }
  }
});
