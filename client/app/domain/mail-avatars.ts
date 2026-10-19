import type { AvatarProps } from "@nuxt/ui";

// What an address wears where its sender publishes no logo (docs/mail.md, 「Sender avatars」): the first
// letter of the name the row shows, in a colour drawn from the address itself, so one address is always
// the same letter in the same colour wherever it stands.

// The letter an identity carries where its name says nothing at all.
const NAMELESS = "?";

// The colours an identity's circle is drawn in. They are dark enough for white lettering, which is what
// the circle carries whatever theme the console is in.
const AVATAR_TONES: readonly string[] = [
  "bg-red-700",
  "bg-orange-700",
  "bg-yellow-700",
  "bg-green-700",
  "bg-teal-700",
  "bg-blue-700",
  "bg-violet-700",
  "bg-pink-700",
  "bg-slate-600"
];

// The first character worth showing of a name: the first letter or digit in it, as one letter.
function initialOf(name: string): string {
  for (const character of name) {
    if (/[\p{Letter}\p{Number}]/u.test(character)) {
      return character.toUpperCase();
    }
  }

  return "";
}

// The tone one identity carries. The same key always answers the same tone.
function avatarTone(key: string): string {
  let hash = 0;

  for (const character of key.toLowerCase()) {
    hash = (hash * 31 + character.codePointAt(0)!) % 1_000_003;
  }

  return AVATAR_TONES[hash % AVATAR_TONES.length]!;
}

// What an identity wears in place of its name: the first letter of its name in a colour drawn from its
// own key, or a question mark on that tone where the name carries no letter at all.
export function identityMark(key: string, name: string): AvatarProps {
  const initial = initialOf(name);

  // The letter takes the circle's own colour rather than the muted one an avatar's fallback carries by
  // default, which on a filled circle is barely there at all.
  return { text: initial === "" ? NAMELESS : initial, class: `${avatarTone(key)} text-white`, ui: { fallback: "text-inherit" }, alt: name };
}
