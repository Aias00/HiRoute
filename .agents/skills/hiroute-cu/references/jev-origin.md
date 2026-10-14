# Jev origin and scope

Parts of `scripts/ax.mjs` and the System One question format in `scripts/jev.mjs`
were adapted from [Sac-Y/Jev-cu](https://github.com/Sac-Y/Jev-cu), originally
revision `12d8e6d1e2bacaa864974cac85443c8176ebcdcb` (`scripts/loop.mjs` and
`scripts/jev-decide.mjs`). HiRoute adds scenario checks, explicit projection,
a shared budget, takeover and its own regression tests.

The upstream now publishes the [MIT license](https://github.com/Sac-Y/Jev-cu/blob/52d32ac24e2cea29c63d9d7c4bd6d4c401111f56/LICENSE),
Copyright (c) 2026 Sac-Y. Preserve [the complete notice](jev-LICENSE.txt) when
redistributing these portions. Earlier package metadata alone is not the
redistribution notice. No upstream installer, other-app examples, historical
runs, environment files or credentials are bundled.

The local runtime uses OpenRouter Decisions API with `typesafe/jev-1.13`, retaining
the question/answer format. It has no dependency on the upstream checkout or
third-party npm packages. Personal environment files remain ignored.
