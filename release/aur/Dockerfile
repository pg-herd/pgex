FROM archlinux:base-devel
ARG PACMAN_FLAGS
RUN pacman $PACMAN_FLAGS -Syu --noconfirm --needed git \
    && useradd --create-home builder
