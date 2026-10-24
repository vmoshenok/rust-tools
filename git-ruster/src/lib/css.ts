/** Preact 11 passes style values to the DOM as-is, so lengths need an explicit unit. */
export const px = (n: number) => `${n}px`;
