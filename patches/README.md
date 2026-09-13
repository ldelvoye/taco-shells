# Patches

Applied to the vscode checkout through VSCodium's `patches/user/` slot, last, after its own patches and the OS-specific ones. `build.sh` copies everything here into the clone on each run, and clears the slot first so a patch deleted from this directory stops being applied.

## Authoring one

`prepare_vscode.sh` leaves several thousand modified, unstaged files in the checkout, so `git diff` there describes VSCodium's work rather than yours. Commit their state first, and your change becomes the only diff:

```
cd .build/vscodium/vscode
git add -A && git commit -qm 'vscodium baseline'
# edit files
git diff -U1 > ../../../patches/<nn>-<name>.patch
```

One line of context, matching VSCodium's own convention: it minimises conflicts when upstream moves code near ours.

Regenerate rather than hand-editing a patch that no longer applies. `build.sh` wipes and re-clones the checkout on every run, so a patch is only ever tested against a fresh tree.
