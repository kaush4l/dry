---
name: components
description: The reply components - which exist, when each one fits, the exact call and arguments, and the mistakes that leave a component out. Load it before you write an answer the person will read.
---
# Components: an answer the app draws

A reply is text, and next to it at most one display (`show=`) and at most
one ask (`ask=`). The app draws them as real controls: a table with
columns, a row of headline numbers, buttons to pick from. Never draw one
in characters (no `| a | b |` rows, no `[####  ]` bars, no "1) 2) 3)" menu
for a choice): write the component and the app draws it, in every shell.

Every argument is one flat string. Inside `data=`, `options=`, `rows=` and
`fields=`, `|` separates the cells of a row (or the options) and `;`
separates rows. So a cell cannot hold `|` or `;`: reword it ("a or b").
A quote inside a value is written `\"`.

## 1. Choose by what the person needs to see

| the answer is | use | not |
|---|---|---|
| one fact, a reason, an explanation | text alone | a one-row table |
| the same properties for several things (planets and their sizes, options and their costs) | `show="table"` | a list with commas inside |
| a few items of one kind, order matters or not (steps, findings, names) | `show="list"` | a table with one column |
| one to four headline numbers, with or without a change | `show="stat"` | a table of two columns |
| how far along something is, as a share | `show="progress"` | "about 70%" in text |
| a style in words (a haiku, for a five-year-old, three sentences, JSON only) | text alone, in that style | any component: the words are the answer |

A component carries the data; `text` carries the answer in one or two
sentences: what the table shows, the conclusion, the caveat. Never repeat
the table's cells in the text. Never leave `text` empty.

## 2. The displays

```text
reply(show="table", data="Planet|Diameter (km)|Moons;Mercury|4,879|0;Venus|12,104|0;Earth|12,756|1", text="The three inner planets, smallest first.")
reply(show="list", data="Install the tool|Run the tests|Read the report", text="Three steps, in order:")
reply(show="stat", data="Revenue|$12.4k|+8%;Users|1204|-3%", text="This week against last week:")
reply(show="progress", data="7/10 tests passed", text="Still running.")
```

- **table**: the first row is the header; every row has as many cells as
  the header. At most 8 columns and 30 rows: more is cut. Put the unit in
  the header ("Diameter (km)"), not in every cell. Sort the rows by what the
  person asked about and say the order in `text`.
- **list**: items separated by `|`, at most 30. One short phrase each.
- **stat**: `label|value|change` per stat, the change is optional. The value
  is written as it should be read ("$12.4k", "41 ms").
- **progress**: `"7/10 tests passed"` or `"70% copied"`: a number, its
  whole, and what is counted.

## 3. The asks

Use one when the answer depends on something only the person knows. The
person gets a control; what they pick comes back as the next message
(`Answer (choice): Green`). One ask per reply.

```text
reply(ask="choice", options="Red|Green|Blue", text="Which colour?")
reply(ask="multi", options="unit|ui|e2e", text="Which tests should I run?")
reply(ask="confirm", text="Delete the old logs?")
reply(ask="number", min="1", max="10", unit="copies", text="How many copies?")
reply(ask="text", placeholder="project name", text="What is the project called?")
reply(ask="date", text="Which day?")
reply(ask="datetime", text="When should I run it?")
reply(ask="review", title="Deploy", rows="env: staging|time: 18:00", text="Is this right?")
reply(ask="form", title="Booking", fields="Day:date|Guests:number(1-12)|Seat:choice(inside,terrace)|Notes:text", text="A few details for the table:")
```

- **choice** needs two or more options; a yes or no is **confirm**.
- Several questions at once are one **form**, never several replies.
- **review** shows details to check before something is done; its rows are
  `label: value`.
- An ask and a display go together in one reply: "here are the options
  (table), which one? (choice)".

## 4. Mistakes that leave a component out

A broken component never fails the reply: it is cut or left out and a note
says why. These are the ones seen:

- `show="table"` with no `data=`, or a header and no rows: nothing shown.
- A row with fewer or more cells than the header: padded or cut.
- A kind that does not exist (`show="chart"`, `ask="select"`): nothing
  shown. The kinds are the ones above, and only those.
- `|` or `;` inside a cell: the cell is split. Reword it.
- Two `show=` or two `ask=` in one reply: only one of each is read.
- The data written in `text` as well: the person reads it twice.

## 5. Before you reply

Read your call once as the app will: count the cells of each row against
the header; check the kind is one of the list; check `text` says something
the component does not. Then reply once.
