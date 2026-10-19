/** A message with plural forms, chosen with `Intl.PluralRules` from the `count` variable. */
export type Plural = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };

export type Message = string | Plural;
