/** Capitalizes the first letter of a string. */
export function capitalize(text) {
  if (!text) return "";
  return text[0].toUpperCase() + text.slice(1);
}

/** Truncates a string to `max` characters, adding an ellipsis. */
export function truncate(text, max) {
  if (text.length <= max) return text;
  return text.slice(0, Math.max(0, max - 1)) + "…";
}
