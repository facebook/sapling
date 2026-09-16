# Review queue home-page mock-up

This static prototype uses local sample data. It does not call GitHub or change the ReviewStack application.

From this directory, run:

```sh
python3 -m http.server 4173
```

Then open <http://127.0.0.1:4173/>.

The section headings and summary counters open the focused queue views:

- <http://127.0.0.1:4173/?view=reviews>
- <http://127.0.0.1:4173/?view=authored>
