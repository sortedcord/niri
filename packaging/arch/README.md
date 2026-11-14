# Arch package

`PKGBUILD` builds this fork as `niri-sortedcord`, which replaces (`conflicts=niri`)
and provides the distro `niri` package. It builds the exact commit pinned in
`_commit`, fetched from https://github.com/sortedcord/niri, so that commit must be
pushed first. `pacman -Syu` never updates it; rebuild to update.

## Build and install a commit

```sh
commit=$(git rev-parse HEAD)          # a pushed commit
pkgver=26.04.r$(git rev-list --count v26.04..$commit).g${commit:0:7}
mkdir -p ~/.cache/niri-pkg && cd ~/.cache/niri-pkg
cp ~/Projects/niri/packaging/arch/PKGBUILD .
sed -i "s/^pkgver=.*/pkgver=$pkgver/; s/^_commit=.*/_commit=$commit/" PKGBUILD
makepkg -f

# Check your config against the new binary before installing.
tar -xOf niri-sortedcord-$pkgver-1-x86_64.pkg.tar.zst usr/bin/niri > /tmp/niri-new
chmod +x /tmp/niri-new && /tmp/niri-new validate --config ~/.config/niri/config.kdl

sudo pacman -U niri-sortedcord-$pkgver-1-x86_64.pkg.tar.zst
```

Then log out and back in. Commit the bumped `pkgver`/`_commit` back here if you
want the repo to record what is installed.

## Roll back to stock niri

Fork-only config syntax (`full-width-to-edges`, `touchpad-gesture`) is rejected by
stock niri, so restore a pre-fork config too:

```sh
cp ~/.config/niri/config.kdl.pre-fork-backup ~/.config/niri/config.kdl
sudo pacman -U /var/cache/pacman/pkg/niri-26.04-1-x86_64.pkg.tar.zst   # or: sudo pacman -S niri
```
