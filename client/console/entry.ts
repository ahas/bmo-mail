import type { ConsoleBundleModule } from "bmo-ui/shared/console-manifest";

import "./style.css";

// The module's default export (bmo-ui's `docs/console-bundles.md`, "What a bundle is"): the mail screen's
// routes under its screen path, `/mail`. The page keeps one instance across every place on the screen, as
// its own page meta keys it in the Nuxt layer.
const bundle: ConsoleBundleModule = {
  routes: [
    {
      path: "/mail/:path(.*)*",
      name: "mail-path",
      // One instance across every place on the screen, under the console's title bar (bmo-ui's
      // `docs/console-bundles.md`, the title bar).
      meta: { key: () => "mail", consoleTitleBar: true },
      component: () => import("../app/pages/mail/[...path].vue")
    }
  ]
};

export default bundle;
