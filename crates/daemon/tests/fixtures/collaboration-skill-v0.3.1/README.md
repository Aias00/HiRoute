# Released collaboration Skill fixtures

Frozen bytes from public v0.3.1 source `13856abd873da5b4373d39322493b4cc105c4867`,
`crates/daemon/src/control/runtime/settings_facts.rs`, constants
`COLLABORATION_SKILL_EXPLICIT_CONTENT` and `COLLABORATION_SKILL_DEFAULT_CONTENT`.

These fixtures are installed through the normal test settings transaction before
current Configure/Apply replaces a HiRoute-owned file. They are never production
input or regenerated from the current template. User-owned/borrowed files retain
the existing conflict boundary. Startup alone does not update an installed Skill.

- `explicit.md`: SHA-256 `e11ca8ebf95e5e685fecb6998aa4452d6e869fcef548a69573aea779084e001b`.
- `default.md`: SHA-256 `1f3519860f3927481a18ba9204d1f2a2d6bfba5288680f7b14f913b53d3a02d3`.
