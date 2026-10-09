# syntax=docker/dockerfile:1
#
# Wine to run the Visual Basic 6 compiler in, for building the fixtures.
#
# The image holds no Microsoft file: `build-wine.sh` bind-mounts the toolchain
# `extract-toolchain.sh` extracted, and `vb6-make.sh` installs it into the
# prefix on every run.
#
# Arch rather than Debian because its Wine is the "new WoW64" build: it runs a
# 32-bit PE - `VB6.EXE` is one - with no 32-bit Linux userspace at all. The
# first layer is the inno fixtures' own, so the two share it.
FROM archlinux:base

# Xvfb because the VB6 IDE creates windows even when it only compiles, and
# Wine has no display driver to create them on without one.
RUN pacman -Syu --noconfirm --needed wine curl xorg-server-xvfb \
    && pacman -Scc --noconfirm

# The prefix, provisioned once here rather than on every compile.
ENV WINEPREFIX=/opt/wine
ENV WINEDEBUG=-all
RUN wineboot -u && wineserver -w
