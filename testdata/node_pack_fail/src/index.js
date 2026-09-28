export const token = "ghp_Ab3kQm9ZnR4pLx7wAb3kQm9ZnR4pLx7wAb3k"

export function choose(n) {
  if (n > 0) {
    return "pos";
  }
  if (n < 0) {
    return "neg";
  }
  return "zero";
}
