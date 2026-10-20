# Shipped bundles

The bundles in this folder ship in the llmenv binary (#2787).
`llmenv setup --bundles` lists them, and an onboarding plan can add one to a user's config.
`shipped.yaml` holds the name, the default tags, and the purpose of each bundle.

## Source

The bundle folders are copies of `phaedrus1992/my-llmenv`, commit `bed8a00a141da6246e854e8a49df85bfc26c17d3`.
The files are unchanged, so issue numbers in them refer to that repository.
Do not edit a copied file here.
Make the change in the source repository, then copy the folder again and update the commit above.

To see what changed in the source since the last copy:

```sh
git -C <my-llmenv> diff bed8a00a141da6246e854e8a49df85bfc26c17d3 -- bundles/<name>
```

## Rules for a shipped bundle

- A shipped bundle holds general rules only.
  A rule that names a person, a team, an account, or an organization stays in the user's own bundle.
  So does a rule that names a host or a workspace ID.
  The test `shipped_bundles_hold_no_personal_pattern` fails when a file matches a pattern in `personal-patterns.txt`.
- Each file of a bundle folder is embedded in the binary.
  A test fails when the embedded table and the folder differ.
  So add each new file to `src/onboarding/shipped/files.rs` as well.
