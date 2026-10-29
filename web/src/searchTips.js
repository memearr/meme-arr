// What the search box understands, shown in the tips popover. The parsing
// itself happens on the server (src/querysyntax.rs) so the API and the UI agree.

export const SEARCH_TIPS = [
  { example: 'show:"Breaking Bad"', says: "only that show (the name just has to contain it)" },
  { example: "-show:friends", says: "leave a show out" },
  { example: "ep:pilot", says: "episodes whose title contains it" },
  { example: "movie:dune", says: "movies whose title contains it" },
  { example: "movie:   or   ep:", says: "with nothing after it: only movies, or only episodes" },
  { example: "library:Anime  season:2", says: "one library, one season number" },
  { example: '"i am the one" -walter', says: "dialogue text, minus lines (or titles) with a word" },
  { example: '-ep:"the pilot"', says: "any operator can be turned around with a leading -" },
];

export const SEARCH_TIPS_FOOTER =
  'Put text in quotes to search it literally, e.g. "show:me the money" or "-5 degrees". ' +
  "Operators work in both Dialogue and Title mode; use as many as you like.";
