# Reference photos

Put the jewelry photos you want to test in this folder. JPG, PNG and WebP work.

To tell the tool what each photo shows, put it in a sub-folder named after
its category:

```
samples/
  ring/        solitaire-01.jpg
  earrings/    studs-07.jpg
  pendant/     heart-03.png
  necklace/
  bracelet/
  bangle/
  brooch/
```

Photos placed directly in `samples/` are treated as general jewelry, unless
you pass `--category ring` (or another category) when you run the tool.

Photos in this folder are ignored by git and never leave this computer,
except when a run sends them to Google's Gemini API.
