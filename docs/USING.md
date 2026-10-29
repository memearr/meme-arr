# Using meme-arr

A tour of the app, in the order you'll use it (new here? Start with
[Getting started](GETTING-STARTED.md), which covers installing and connecting your
server). Every picture here was taken against
a **made-up library** (shows like *The Lantern Cove* and *Orbital Diner*, with
invented dialogue) served by a pretend Plex server, so nothing in them comes from a
real collection. You can build the same library yourself in a minute; see
[Try it with the mock library](#try-it-with-the-mock-library).

## Find a moment

Type a line you remember into the search box and press **Search**. *Dialogue* looks
inside the subtitles that were indexed; *Title* looks at show, episode and movie
names. Each result is one line of dialogue, with the show, episode and the moment it
is said.

![Dialogue search for "soup": results from several shows, with the library overview above](images/search-dialogue.png)

- The cards at the top summarise each library: how many items are indexed and how
  many are still **missing** subtitles.
- Results come 50 at a time. The list says "Showing the first 50 matches" and a
  **Show more** button loads the next page; a finished list says "95 matches".
- When there is more than one library, **Search in** limits the search to any
  subset of them. Tick as many as you like; nothing ticked means all of them.

![The Search in picker with two libraries ticked](images/search-library-picker.png)

## Narrow it first, then search inside it

Under the search box is a filter bar. **+ Filter** adds a filter (a show, an episode
title, a movie title, a season number, only movies, only episodes, or the opposite of
any of them with *Exclude these instead*), and **Search in** picks libraries. Each filter
becomes a chip that **stays** while you search, so you can narrow to a subset first and
then search the dialogue *inside* it. Remove a chip with its **×**.

![Adding a filter: narrow to a show](images/search-add-filter.png)

With a filter set and nothing typed, the page lists the items the filter leaves; type some
dialogue and it searches only those. Change the text as often as you like; the filter stays
until you remove it (it also applies in Title mode, and survives a visit to another tab).

![A dialogue search for "soup" inside the show "Orbital Diner"](images/search-filter-chips.png)

In the **Library** tab, a show's episode list has **Search dialogue in this show**: it
applies that show as a filter and puts the cursor in the search box.

## Narrow it down like a web search

You can also put operators in the search box itself; a filter chip is just a friendlier way
to add the same thing, and the two work together. Whatever is left over is the text that gets
searched, so you can still search the dialogue (or the title) at the same time.

| Type | Meaning |
|---|---|
| `show:"Orbital Diner"` | only that show (its name just has to contain the text) |
| `-show:"Lantern Cove"` | leave a show out (movies, which have no show, are kept) |
| `ep:docking` | episodes whose title contains "docking" |
| `movie:lighthouse` | movies whose title contains "lighthouse" |
| `movie:` or `ep:` on its own | only movies, or only episodes |
| `library:Cartoons`, `season:2` | one library, one season number |
| `-walter`, `-"say my name"` | leave out dialogue lines (or titles) containing a word or phrase |
| a leading `-` on any operator | turn it around, e.g. `-ep:docking` |

For example, `soup show:"Orbital Diner" -ep:docking` finds the soup lines in
*Orbital Diner*, except from the "Docking Fees" episode. The page reads back what it
understood ("Using show: Orbital Diner · not episode: docking"):

![A dialogue search with two operators; the chips under the picker show how they were read](images/search-operators.png)

The **?** next to the search box lists them all:

![The search tips popover](images/search-tips.png)

In **Title** mode the same operators narrow by show and episode title, which is
where they help most: `show:"Lantern Cove" season:2 -ep:gull` lists the second
season's episodes except the gull one.

![A title search narrowed with show:, season: and -ep:](images/search-title.png)

A few things worth knowing:

- Put text in quotes to search it literally: `"show:me the money"` and
  `"-5 degrees"` are words, not operators. Anything with a colon that isn't an
  operator (`note: hello`, `12:30`) is ordinary text too.
- With operators but **no dialogue text** (just `show:"Orbital Diner"`), Dialogue
  mode lists the matching *items* instead of nothing, with a note saying so.
- Excluding a word in Dialogue mode removes only the *lines* that contain it, and
  needs some dialogue text to search for. There is no OR and no grouping.

## Make the clip

Click a result. The editor opens with a preview of that moment, already trimmed to the
line with a little padding. Nudge the in and out points, choose captions (none, the
real subtitles, or your own top and bottom text), pick a format (WebP, MP4, WebM or
GIF), size and quality, and export. Tick **Include audio** to keep the clip's sound (MP4
and WebM only; GIF and WebP can't carry audio). Finished files also land in your output folder.

![The editor: preview with the caption burned in, trim controls, caption and format choices, and the editable subtitle lines](images/editor.png)

![The caption choices with the font picker and the text size slider](images/editor-fonts.png)

- **Burn subtitles** puts the indexed line on the clip. Each line is editable: fix a
  typo, shorten it, or drop it. **Subtitle sync** shifts the timing if this release
  of the video drifts from the subtitle file.
- **Font and text size.** With burned subtitles or meme text selected, **Font** picks the
  typeface and **Text size** scales the text (50% to 300%; 100% is normal). Your choice is
  remembered for next time, and each clip in a sequence keeps its own. Add fonts under
  [Settings](#settings). Reload the preview to see a change.
- **Exports are clean.** Nothing from the original file is copied into a meme: not its
  release name or other tags, comments, chapters, creation dates, track names or
  subtitle tracks. The one thing a file may carry is the movie or show/episode it came
  from, as its `title` (MP4 and WebM; untick **Name the source in the file's title** to
  leave even that out).
- **Find subtitles online** (above the trim controls) searches your media server's
  own subtitle sources, if the item has no subtitles or the wrong ones; see
  [Find missing subtitles](#find-missing-subtitles).

### Sequences

**Add to sequence** queues a clip. The **Sequence** tab joins several clips, from
any shows or movies, into one back-to-back GIF, WebP, MP4 or WebM (up to 20 clips and
3 minutes). Reorder or remove clips, choose the format, and export.

![The Sequence tab with three clips from different shows, ready to export as one GIF](images/sequence.png)

## Browse the library

The **Library** tab lists every movie and show, with how many episodes are indexed and
how many are missing subtitles. Click a show to open its episodes.

Two filters narrow the list, and they combine with the title box:

- **Libraries** (shown when you have more than one): tick any subset, or leave it on
  *All libraries*.
- **All / Missing subtitles / Fully indexed**: *Missing subtitles* lists the movies and
  shows that still have at least one item without subtitles; *Fully indexed* lists the
  ones with nothing missing.

![The Library tab filtered to items that are missing subtitles](images/library-missing.png)

![The Library tab: movies and shows with their subtitle status](images/library.png)

## Find missing subtitles

Wherever the app says something has **no subtitles** or **N missing**, that notation is
a link: click it and you land on **Wanted** with exactly that pulled up.

| You click | You land on |
|---|---|
| **N missing** on a library card (Make) | Wanted, with just that library selected |
| **Find subtitles** next to an item in a library's *Missing subtitles* list (Make) | Wanted, with that item's subtitle search open and ready |
| **N missing** on a show card (Library tab) | Wanted, filtered to that show's missing episodes (if only one is missing, its search opens) |
| **no subtitles** on a movie card (Library tab) | Wanted, with that movie's search open |
| **no subs →** next to an episode in a show's episode list | Wanted, with that episode's search open |
| **no subtitles →** on a result in Title search (Make) | Wanted, with that item's search open |

(The cards themselves still open the item or the show, as before.) If an item isn't
on the Wanted list after all, because it has subtitles now or its file can't be
reached, Wanted says so instead of leaving you guessing.

Items with no searchable subtitles are listed under **Wanted**. The same multi-library
picker as the search narrows the list, and the box next to it filters by title.

![The Wanted tab with the library picker open](images/wanted-library-picker.png)

**Find subtitles** on a row searches the online sources your media server uses (Plex's
built-in search, or Jellyfin's subtitle providers). Each result shows the release
name, the source, the format and how well it matches.

![Online subtitle search results for one episode](images/subtitle-search.png)

**Download** asks the server to fetch it. The server does that in the background, so
the download is tracked on the **Requests** tab until it becomes searchable. Plex
keeps a downloaded subtitle in its own database rather than as a file; **Re-index**
reads it through Plex and indexes it (and a scheduled task re-checks waiting requests
every hour, so you don't have to press it). If you'd also like the `.srt` file next
to the video, turn on *Also save it as a .srt file next to the video* in
**Settings**.

A request that stays **Waiting** has a **Diagnose** button. It compares what this app
can see (the video, the subtitle files beside it) with what the server says it has,
and says the likely cause in plain words:

![The Requests tab: one request indexed, two waiting, and the Diagnose report for one of them](images/requests-diagnose.png)

## Keep an eye on it

**System** shows health checks (each says how to fix a problem), what the media
server supports, index statistics and the scheduled tasks, with last and next run and
a *Run now* button for each. **Activity** is the log of what the app has been doing:
indexing, subtitle requests, errors.

![The System tab: health, online subtitle search support, statistics and tasks](images/system.png)

![The Activity tab: indexing and the subtitle request log](images/activity.png)

## Settings

**Settings** holds the media server connection, the **library path mappings** (which
folder the server reports, and where the same folder is mounted for this app), the
**caption fonts**, login options, the API key and trusted proxies.

**Caption fonts** lists the fonts captions can use. The ones that come with the app are
marked *built in*: DejaVu Sans (the default) and the open-source Anton, Bebas Neue, Bangers,
Comic Neue and Luckiest Guy. **Upload a font** adds your own TrueType or OpenType file (`.ttf` or
`.otf`, up to 16 MB, up to 50 fonts); it then appears in the editor's **Font** picker. Delete an uploaded font with its
**Delete** button. Only upload fonts you have the right to use.

![The Caption fonts panel in Settings](images/settings-fonts.png)

![The Settings tab: media server, library path mappings and the optional .srt saving](images/settings.png)

## Dark theme and phones

The theme follows your system (or switch it from the sidebar), and every tab works on
a phone.

![Dialogue search in the dark theme](images/search-dark.png)

![Dialogue search on a phone](images/search-phone.png)

## Try it with the mock library

The pictures above come from a mock library in `dev/mock/`: 46 invented items
(about 3 MB of placeholder video plus subtitles) and a pretend Plex server. It is also
a handy way to try the app, or test a change, without a real library. See
[Development](DEVELOPMENT.md#mock-library-and-screenshots) for how to run it and how
the screenshots are regenerated.
