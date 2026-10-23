import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import vue from "@vitejs/plugin-vue";
import AutoImport from "unplugin-auto-import/vite";
import Components from "unplugin-vue-components/vite";
import { defineConfig } from "vite";

import type { Plugin } from "vite";

// The mail screen as the browser module a console loads (docs/api.md, "The console plugin bundle"), built
// from the same sources the Nuxt layer packs. Every import of what a console shares — `vue`, `vue-router`,
// `vue-i18n`, `nuxt/app`, `@nuxt/ui` and bmo-ui — stays an import of the package's own path, which the
// console resolves to its copy; everything else is carried in the module. The names Nuxt imports and
// registers by itself are read from the declarations `nuxi prepare` writes, so the module imports
// exactly what the layer would have, each from its package's own path. The output is three files: the
// module, its stylesheet and its words, which the service answers.

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const generated = join(root, ".nuxt");
const packages = join(root, "node_modules");
const out = process.env.BMO_MAIL_CONSOLE_OUT ?? join(root, ".output", "console");

// What a console shares with every bundle, by the package's own path.
const SHARED = /^(?:vue|vue-router|vue-i18n|nuxt\/app|@nuxt\/ui(?:\/.*)?|bmo-ui\/.+)$/;

// The page macro Nuxt strips from a page as it builds; the route's meta stands in the module's routes.
const PAGE_MACRO = join(here, "page-meta.ts");

// The names `nuxt/app` exports, read from its own entry.
function nuxtAppExports(): Set<string> {
  const entry = readFileSync(join(packages, "nuxt", "dist", "app", "index.js"), "utf8");
  const names = [...entry.matchAll(/export\s*\{([^}]*)\}/g)].flatMap((found) => found[1]!.split(","));

  return new Set(names.map((name) => name.trim().split(/\s+as\s+/).pop()!).filter((name) => name !== ""));
}

const NUXT_APP = nuxtAppExports();

// Where a name Nuxt's declarations import from is taken from in the module: the shared package's own path,
// the bundle's own file, or nothing where it is neither.
function sourceOf(name: string, from: string): string | null {
  if (from === "vue" || from === "vue-router" || from === "vue-i18n") return from;
  if (from.startsWith("#app/")) return NUXT_APP.has(name) ? "nuxt/app" : null;

  const file = resolve(generated, from);
  const inPackages = relative(packages, file);

  if (!inPackages.startsWith("..")) {
    const ui = inPackages.match(/^@nuxt\/ui\/dist\/runtime\/(.+)$/);

    if (ui !== null) return `@nuxt/ui/${ui[1]}`;
    if (inPackages.startsWith("bmo-ui/")) return inPackages;

    return null;
  }

  return relative(root, file).startsWith("..") ? null : file;
}

interface Declared {
  name: string;
  from: string;
}

// The names `.nuxt/imports.d.ts` declares, each with the module it comes from.
function declaredImports(): Declared[] {
  const text = readFileSync(join(generated, "imports.d.ts"), "utf8");

  return [...text.matchAll(/export\s*\{([^}]*)\}\s*from\s*'([^']+)'/g)].flatMap((found) =>
    found[1]!
      .split(",")
      .map((name) => name.trim())
      .filter((name) => name !== "" && !name.startsWith("type "))
      .map((name) => ({ name, from: found[2]! }))
  );
}

// The components `.nuxt/components.d.ts` declares, each with the file it is.
function declaredComponents(): Declared[] {
  const text = readFileSync(join(generated, "components.d.ts"), "utf8");

  return [...text.matchAll(/^export const (\w+): typeof import\("([^"]+)"\)\['default'\]$/gm)].map((found) => ({
    name: found[1]!,
    from: found[2]!
  }));
}

// Every file of the bundle's own sources a name could be used in.
function sources(folder: string): string[] {
  return readdirSync(folder, { withFileTypes: true, recursive: true })
    .filter((entry) => entry.isFile() && /\.(?:vue|ts)$/.test(entry.name))
    .map((entry) => join(entry.parentPath, entry.name));
}

// Nuxt's `$fetch` is the console's own fetcher, standing on the page as a global; the page macro is the
// bundle's no-op. Everything else Nuxt imports by itself is taken from where `sourceOf` finds it.
const imports = new Map<string, string>([["definePageMeta", PAGE_MACRO]]);
const components = new Map<string, string>();
const unshared: string[] = [];

for (const { name, from } of declaredImports()) {
  if (name === "$fetch" || imports.has(name)) continue;

  const source = sourceOf(name, from);

  if (source === null) unshared.push(name);
  else imports.set(name, source);
}

for (const { name, from } of declaredComponents()) {
  const source = sourceOf("default", from);

  if (source === null) unshared.push(name);
  else components.set(name, source);
}

// A module that used a name a console does not share would fail in the console's page, so the build
// refuses it first.
function sharedOnly(): Plugin {
  return {
    name: "bmo-mail-shared-only",
    buildStart() {
      const files = [...sources(join(root, "app")), ...sources(join(root, "shared"))];
      const used = unshared.filter((name) =>
        files.some((file) => new RegExp(`(?<![\\w$.])${name.replace(/\$/g, "\\$")}(?![\\w$])`).test(readFileSync(file, "utf8")))
      );

      if (used.length > 0) this.error(`The screen uses what a console does not share: ${used.join(", ")}`);
    }
  };
}

// The layer's stylesheet as the module's stylesheet reads it: its theme, variants and utilities with none
// of the sources it scans, so the stylesheet carries the utilities of the screen's own sources alone and
// none of the layer's or the component library's, which the console's own stylesheet carries. Each file
// on the way is copied beside the generated project with its `@source` lines left out and its imports
// pointed at the copies.
const THEME = join(generated, "console-theme");

function withoutSources(text: string): string {
  return text.replace(/@source[^;]*;/g, "");
}

function writeTheme(): void {
  const ui = join(packages, "@nuxt", "ui", "dist", "runtime");
  const layer = join(packages, "bmo-ui", "app", "assets", "css", "main.css");

  mkdirSync(THEME, { recursive: true });
  writeFileSync(join(THEME, "ui.css"), withoutSources(readFileSync(join(generated, "ui.css"), "utf8")));
  writeFileSync(
    join(THEME, "nuxt-ui.css"),
    withoutSources(readFileSync(join(ui, "index.css"), "utf8"))
      .replace('@import "#build/ui.css"', `@import "${join(THEME, "ui.css")}"`)
      .replace('@import "./keyframes.css"', `@import "${join(ui, "keyframes.css")}"`)
  );
  writeFileSync(
    join(THEME, "layer.css"),
    withoutSources(readFileSync(layer, "utf8"))
      .replace('@import "tailwindcss";', '@import "tailwindcss" source(none);')
      .replace('@import "@nuxt/ui";', `@import "${join(THEME, "nuxt-ui.css")}";`)
  );
}

writeTheme();

// The bundle's words, one JSON object keyed by locale, each locale's tree from its file in the language
// pack.
function words(): Plugin {
  const folder = join(root, "i18n", "locales");

  return {
    name: "bmo-mail-words",
    generateBundle() {
      const tree = Object.fromEntries(
        readdirSync(folder)
          .filter((file) => file.endsWith(".json"))
          .sort()
          .map((file) => [basename(file, ".json"), JSON.parse(readFileSync(join(folder, file), "utf8")) as unknown])
      );

      this.emitFile({ type: "asset", fileName: "words.json", source: `${JSON.stringify(tree)}\n` });
    }
  };
}

export default defineConfig({
  root,
  logLevel: "warn",
  plugins: [
    sharedOnly(),
    vue({ isProduction: true }),
    AutoImport({
      imports: [...imports].map(([name, from]) => ({ from, imports: [name] })),
      include: [/\.[cm]?ts$/, /\.vue$/, /\.vue\?vue/],
      dts: false,
      vueTemplate: true
    }),
    Components({
      dirs: [],
      dts: false,
      resolvers: [(name: string) => components.get(name)]
    }),
    tailwindcss(),
    words()
  ],
  resolve: {
    alias: [{ find: /^#console-theme$/, replacement: join(THEME, "layer.css") }]
  },
  define: {
    "import.meta.server": "false",
    "import.meta.client": "true",
    "import.meta.dev": "false",
    "process.env.NODE_ENV": JSON.stringify("production"),
    __VUE_OPTIONS_API__: "true",
    __VUE_PROD_DEVTOOLS__: "false",
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: "false"
  },
  build: {
    outDir: out,
    emptyOutDir: true,
    target: "es2022",
    minify: true,
    sourcemap: false,
    copyPublicDir: false,
    cssCodeSplit: false,
    lib: {
      entry: join(here, "entry.ts"),
      formats: ["es"],
      fileName: () => "module.js",
      cssFileName: "style"
    },
    rollupOptions: {
      external: (id) => SHARED.test(id),
      output: { codeSplitting: false }
    }
  }
});
