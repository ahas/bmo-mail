// The page macro as the module takes it: Nuxt strips the call from a page as it builds, and the module's
// routes carry the page's meta themselves (`entry.ts`).
export function definePageMeta(_meta: Record<string, unknown>): void {}
