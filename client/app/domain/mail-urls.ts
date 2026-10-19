// Everything of the console lives under one base path. $fetch already knows it; an <img> and an
// <iframe> are given the whole path themselves.
export function appUrl(path: string): string {
  const base = useRuntimeConfig().app.baseURL;

  return `${base.replace(/\/$/, "")}${path}`;
}
