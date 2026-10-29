"""A made-up media library for demos, screenshots and manual testing.

Everything here is invented (shows, movies and dialogue), so it can be shown
publicly. Three libraries, 46 items; six have no subtitles on purpose so the Wanted tab has
something to show, and a few lines repeat across shows ("soup", "the door") so
operators like `show:` and `-show:` have something to narrow.

`items()` yields one dict per playable item; gen_media.py turns them into tiny
video files plus .srt sidecars and a manifest, and mock_plex.py serves them as a
Plex server would.
"""

# Lines that recur, so a search finds several shows.
SOUP = "I have a bad feeling about the soup."
DOOR = "Somebody is at the door, and it's not the delivery person."

SHOWS = [
    {
        "library": "TV Shows",
        "show": "The Lantern Cove",
        "year": 2018,
        "seasons": {
            1: [
                ("Low Tide", ["Welcome to Lantern Cove, where the tide is always late.",
                              "Well, that's the tide for you.",
                              "Nobody leaves the cove before the pie is cut.",
                              "I have a bad feeling about the chowder."]),
                ("The Missing Ferry", ["The ferry left at noon, and so did my patience.",
                                       "Well, that's the tide for you.",
                                       "Has anyone checked the lighthouse? Anyone? Anyone at all?",
                                       DOOR]),
                ("Fog Day", ["You can't see the harbor, but you can hear it gossiping.",
                             "Fog is just the sea thinking out loud.",
                             "Nobody leaves the cove before the pie is cut.",
                             SOUP]),
                ("Pie Contest", ["Rule one of the pie contest: no one talks about the pie contest.",
                                 "Is that a lattice or a cry for help?",
                                 "Well, that's the tide for you.",
                                 "The judges are bribable, but only with more pie."]),
                ("Lighthouse Keys", ["Somebody has to hold the keys, and it is never going to be Dale.",
                                     "I swear the lighthouse winked at me.",
                                     DOOR,
                                     "We are not having this conversation in the dark."]),
                ("Last Boat Home", ["The last boat leaves at dusk. Don't be romantic about it.",
                                    "Fog is just the sea thinking out loud.",
                                    "Well, that's the tide for you.",
                                    "Goodnight, Lantern Cove. Try not to float away."]),
            ],
            2: [
                ("Season Opener", ["Another summer, another crisis involving gulls.",
                                   "Well, that's the tide for you.",
                                   "Welcome back, everyone. The pie is still undefeated."]),
                ("The Gull Summit", ["We are here to discuss the gulls. The gulls are here to discuss lunch.",
                                     "I move that we ban french fries.",
                                     "Motion denied. By the gulls."]),
                ("Tideline", ["The tide went out and took the ferry schedule with it.",
                              "Well, that's the tide for you.",
                              "Nobody leaves the cove before the pie is cut."]),
                ("Storm Warning", ["The barometer is doing that thing again.",
                                   "Everyone to the lighthouse, and bring snacks.",
                                   "I have a bad feeling about the thunder."]),
            ],
        },
    },
    {
        "library": "TV Shows",
        "show": "Orbital Diner",
        "year": 2021,
        "seasons": {
            1: [
                ("Zero-G Special", ["Zero gravity, full flavor.",
                                    "Order up! Please catch it.",
                                    SOUP,
                                    "Refills are free. Gravity is extra."]),
                ("Docking Fees", ["Parking a spaceship here costs more than the meal.",
                                  "Zero gravity, full flavor.",
                                  "I'm not paying for the meteor, I only ordered the soup.",
                                  DOOR]),
                ("The Soup Incident", ["Nobody panic, the soup is only slightly sentient.",
                                       SOUP,
                                       "It looked at me. The soup looked at me.",
                                       "Table four wants to speak to the manager. And the soup."]),
                ("Night Shift", ["The night shift is just the day shift with worse lighting.",
                                 "Zero gravity, full flavor.",
                                 "Somebody left a moon on the counter again."]),
                ("Free Refills", ["Unlimited refills means unlimited. Do not test me.",
                                  "He drank a nebula. Cash only.",
                                  "Zero gravity, full flavor."]),
                ("The Regular", ["He has ordered the same thing for nine years. Nobody knows his name.",
                                 "His name is Gerald and he is from the future.",
                                 SOUP]),
                ("Cosmic Brunch", ["Brunch is a state of mind and also a very large asteroid.",
                                   "Order up! Please catch it.",
                                   "I'll have whatever the comet is having."]),
                ("Last Call", ["Last call for the last call.",
                               "Zero gravity, full flavor.",
                               "Lock the airlock on your way out. Or don't. We'll see."]),
            ],
        },
    },
    {
        "library": "TV Shows",
        "show": "Detective Moss",
        "year": 2016,
        "seasons": {
            1: [
                ("The Case of the Quiet Parrot", ["Elementary is overrated, I prefer obvious.",
                                                   "The parrot knows something. The parrot always knows something.",
                                                   DOOR]),
                ("A Study in Beige", ["Everything in this house is beige. That is the clue.",
                                      "Elementary is overrated, I prefer obvious.",
                                      "Who paints a murder scene beige?"]),
                ("The Locked Pantry", ["The pantry was locked from the inside. By the pantry.",
                                       "I have a bad feeling about the jam.",
                                       "Elementary is overrated, I prefer obvious."]),
                ("Mossy Reasoning", ["I deduce that you did it. I deduce that I'm hungry.",
                                     "The alibi is airtight. The alibi is also a goldfish.",
                                     "Elementary is overrated, I prefer obvious."]),
                ("The Double Cross", ["Two people, one coat, and a lot of explaining to do.",
                                      "Never trust a man who owns three umbrellas.",
                                      DOOR]),
                ("Case Closed, Mostly", ["The case is closed. Mostly. There's a lingering question about the parrot.",
                                         "Elementary is overrated, I prefer obvious.",
                                         "Goodnight, Detective."]),
            ],
        },
    },
    {
        "library": "Cartoons",
        "show": "Pocket Dragons",
        "year": 2019,
        "seasons": {
            1: [
                ("Small Fire, Big Plans", ["Do not feed the dragons after midnight. Or before. Or ever.",
                                           "Small fire, big plans!",
                                           "That was not a smoke alarm. That was a battle cry."]),
                ("Hoard Day", ["It's not hoarding if the pile is organized.",
                               "Small fire, big plans!",
                               "That's my spoon. It's also my spoon's spoon."]),
                ("The Great Sock Heist", ["Socks go missing. Dragons go smug.",
                                          "Small fire, big plans!",
                                          DOOR]),
                ("Nap Time Negotiations", ["Five more minutes, said the dragon who needed five more hours.",
                                           "Small fire, big plans!"]),
                ("Scales of Justice", ["The court will now hear the case of the missing marshmallow.",
                                       "I plead the fifth snack.",
                                       "Small fire, big plans!"]),
                ("Pocket Full of Sparks", ["We saved the village. And also accidentally set the village on fire.",
                                           "Small fire, big plans!",
                                           SOUP]),
            ],
        },
    },
    {
        "library": "Cartoons",
        "show": "Captain Quackers",
        "year": 2014,
        "seasons": {
            1: [
                ("Anchors Aweigh, Mostly", ["Anchors aweigh! Mostly aweigh. Some of the anchors are staying.",
                                            "Quack to the future!",
                                            "Steady as she goes, which is to say, sideways."]),
                ("Treasure of the Bathtub", ["The map says X marks the bathtub.",
                                             "Quack to the future!",
                                             "It's not a bubble, it's a sea monster."]),
                ("The Kraken Is Shy", ["Be gentle. The kraken is shy.",
                                       "Quack to the future!",
                                       DOOR]),
                ("Mutiny on the Pond", ["You can't mutiny against a duck. Technically you can.",
                                        "Quack to the future!"]),
                ("Home Port", ["There's no place like the pond.",
                               "Quack to the future!",
                               "Goodnight, crew. Try not to sink."]),
            ],
        },
    },
]

MOVIES = [
    ("Paper Moon Heist", 2019, ["The moon is made of paper. I checked. Twice.",
                                "Every great heist starts with a really good snack.",
                                "Nobody steals the moon on a school night.",
                                SOUP]),
    ("The Last Lighthouse", 2021, ["Somebody has to keep the light on.",
                                   "The sea doesn't care about your plans.",
                                   "We leave at first light. Or whenever the light is feeling better.",
                                   DOOR]),
    ("Midnight Mixtape", 2017, ["Side A is for dancing. Side B is for regrets.",
                                "Rewind it. Rewind it again.",
                                "Some songs are only for the drive home."]),
    ("Sundae Rush", 2022, ["Three scoops, no regrets, one very sticky escape.",
                           "The sprinkles were a mistake. The sprinkles are always a mistake.",
                           "Run! The ice cream truck knows our names!"]),
    ("Quiet Thunder", 2015, ["The thunder only whispers when it means it.",
                             "Don't wake the storm, it's grumpy.",
                             "I've been listening to the quiet all my life."]),
    ("The Velvet Detour", 2020, ["This is not the way to the airport.",
                                 "It's the scenic route. The very, very scenic route.",
                                 DOOR]),
    ("Cactus Kingdom", 2018, ["Long live the prickly queen.",
                              "It's a dry heat, and a drier court.",
                              "Water is just a rumor in this kingdom."]),
    ("Brass Band Blues", 2016, ["One more chorus, then we all go home. We won't go home.",
                                "That trombone has seen things.",
                                "Play it like you mean it. Play it like the rent is due."]),
    ("A Very Small Spaceship", 2023, ["It's not that the ship is small. It's that space is large.",
                                      "Request permission to be slightly lost.",
                                      "Permission granted. Please remain slightly lost."]),
    ("Lemonade Stand-Off", 2014, ["Two stands. One street. Zero chill.",
                                  "Fifty cents, and you get a lecture on citrus.",
                                  "Everybody out of the pool, the lemons are coming."]),
    ("No Soup for You, Probably", 2013, ["I came here for the soup, and the soup left without me.",
                                          SOUP,
                                          "The ladle knows what it did."]),
]

# Items left without subtitles, for the Wanted tab (by title).
NO_SUBTITLES = {
    "Tideline",
    "A Study in Beige",
    "Nap Time Negotiations",
    "Mutiny on the Pond",
    "Cactus Kingdom",
    "Lemonade Stand-Off",
}

# How long each generated clip is, and when the lines appear.
CLIP_SECONDS = 36
FIRST_LINE_AT = 3.0
LINE_EVERY = 7.0
LINE_LENGTH = 3.5


def items():
    """Every item as a dict: rating_key, library, kind, title, show, season, episode, year,
    lines (None when it has no subtitles) and downloadable (what an online search would find)."""
    n = 0
    libs = {"TV Shows": "1", "Cartoons": "2", "Movies": "3"}
    for s in SHOWS:
        for season, eps in s["seasons"].items():
            for i, (title, lines) in enumerate(eps, start=1):
                n += 1
                yield {
                    "rating_key": f"{1000 + n}",
                    "library": s["library"],
                    "section": libs[s["library"]],
                    "kind": "episode",
                    "title": title,
                    "show": s["show"],
                    "season": season,
                    "episode": i,
                    "year": s["year"],
                    "lines": None if title in NO_SUBTITLES else lines,
                    "downloadable": lines,
                }
    for title, year, lines in MOVIES:
        n += 1
        yield {
            "rating_key": f"{1000 + n}",
            "library": "Movies",
            "section": libs["Movies"],
            "kind": "movie",
            "title": title,
            "show": None,
            "season": None,
            "episode": None,
            "year": year,
            "lines": None if title in NO_SUBTITLES else lines,
            "downloadable": lines,
        }


def srt(lines):
    """The subtitle file for an item's lines."""
    out = []
    for i, text in enumerate(lines, start=1):
        start = FIRST_LINE_AT + (i - 1) * LINE_EVERY
        end = start + LINE_LENGTH
        out.append(f"{i}\n{ts(start)} --> {ts(end)}\n{text}\n")
    return "\n".join(out) + "\n"


def ts(seconds):
    ms = int(round(seconds * 1000))
    h, ms = divmod(ms, 3_600_000)
    m, ms = divmod(ms, 60_000)
    s, ms = divmod(ms, 1000)
    return f"{h:02d}:{m:02d}:{s:02d},{ms:03d}"


if __name__ == "__main__":
    all_items = list(items())
    without = [i for i in all_items if not i["lines"]]
    assert len(without) == len(NO_SUBTITLES), "every NO_SUBTITLES title must match an item"
    print(f"{len(all_items)} items, {len(without)} without subtitles")
