This is a program for n-player chess clocks.

You create a clock and get an identifier you can share with others.
Then someone starts the clock, and whomever's clock is running clicks the button,
and it starts the next player's clock. When someone's clock runs out, the flag falls.

A new clock requires the total time for each player and the number of players.
It creates a random identifier so all players can go to the same clock.

There isn't (yet) any assignment of timers to players:
you just have to work that out yourself and make sure to only hit your own button.

# Database Schema

```
clocks
------
id
player_count  - 1 or more
allowed_time  - in positive seconds
sharing_code  - generated from the id
started_at    - null at first

spans
-----
id
clock_id
player_number     - not null, indexed from 0
started_at        - not null
starting_seconds  - not null
finished_at       - null until you create the next span or the clock runs out
finishing_seconds - null until you create the next span or the clock runs out

index spans (clock_id, started_at)
index spans (clock_id, player_number, started_at)
```

Clicking a clock should UPDATE the currently-open span and INSERT a new span.
The old span's `finished_at` must equal the new span's `started_at`.


# Frontend

The website has a form to ask for a new clock.
It POSTs the inputs, and you get redirected to your new clock page.
You can share the URL with others.

Each player's clock has a button, but only one is enabled at a time.
Initially the *last* player's clock's button is enabled, and the clock is not running.

The frontend can POST a "hit" for a given player, and it starts the clock for the next player.
That is an Ajax JSON call that takes the `sharing_code` and `player_number`.
It returns a bare success message.

Each player's frontend uses a websocket to ask for clock updates all the time.
Each update includes the remaining seconds for each player.


# Backend

When need some data structure that is shared between the main webserver (that receives a POST when a click is hit) and the websocket that reports times.
The main webserver will write to it, and the websocket threads will read from it.
It has a "hit" function that records the stop & start time.
It also adds a new database record.
I'm not really sure what the database stuff is even for. Just if the server goes down?
Maybe I don't need a database at all, and it just stays in memory?
It could just write out to a log file so I can see what's happening.
Of course then I also need a mapping from `sharing_codes` to clocks,
but that doesn't seem too hard.

