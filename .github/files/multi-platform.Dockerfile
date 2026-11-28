# Builds the glibc (default), full and static (-legacycpu) images from Wolfi packages on scratch, keeping the apk database for image scanners.

# Runs on the build platform and installs the target platform's packages, so arm64 images build without emulation.
FROM --platform=$BUILDPLATFORM cgr.dev/chainguard/wolfi-base:latest@sha256:4cf5932a0f95a88af352f77e4efa67480cab0e1f7a48f0eddca3cce003a7c0fb AS wolfi
ARG TARGETARCH
# Installs into /rootfs from the Wolfi repository instead of wolfi-base's default Chainguard one, skipping package scripts because /rootfs has no shell.
ENV APK="apk add --no-cache --no-scripts --initdb --root /rootfs --keys-dir /etc/apk/keys --repository https://packages.wolfi.dev/os --arch"
# Creates the home of `nonroot` (uid and gid 65532, as in Chainguard and distroless images) and maps Docker's architecture name to apk's.
RUN mkdir -p /rootfs/etc /rootfs/home/nonroot \
    && chown 65532:65532 /rootfs/home/nonroot \
    && echo "$TARGETARCH" | sed 's/^amd64$/x86_64/; s/^arm64$/aarch64/' > /apk-arch

FROM wolfi AS rootfs-static
RUN $APK "$(cat /apk-arch)" wolfi-baselayout ca-certificates-bundle tzdata \
    && echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /rootfs/etc/passwd \
    && echo 'nonroot:x:65532:' >> /rootfs/etc/group

# Rust's linux-gnu binaries load glibc and libgcc_s.so.1 from libgcc, which unwinds panics.
FROM wolfi AS rootfs-glibc
RUN $APK "$(cat /apk-arch)" wolfi-baselayout ca-certificates-bundle tzdata glibc libgcc \
    && echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /rootfs/etc/passwd \
    && echo 'nonroot:x:65532:' >> /rootfs/etc/group

FROM wolfi AS rootfs-full
# Drops the Vulkan drivers, which cannot load without a Vulkan loader, because rendering uses llvmpipe through libEGL.so.1.
RUN $APK "$(cat /apk-arch)" wolfi-baselayout ca-certificates-bundle tzdata glibc libgcc mesa-egl mesa-gles mesa-libgallium \
    && rm -f /rootfs/usr/lib/libvulkan_*.so /rootfs/usr/share/vulkan/icd.d/*.json \
    && echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /rootfs/etc/passwd \
    && echo 'nonroot:x:65532:' >> /rootfs/etc/group

FROM scratch AS full
ARG TARGETPLATFORM
LABEL org.opencontainers.image.source="https://github.com/maplibre/martin"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"
LABEL org.opencontainers.image.documentation="https://maplibre.org/martin/"
LABEL org.opencontainers.image.vendor="maplibre"
LABEL org.opencontainers.image.authors="Yuri Astrakhan, Stepan Kuzmin and MapLibre contributors"
COPY --from=rootfs-full /rootfs/ /
COPY target_releases/$TARGETPLATFORM/* /usr/local/bin/
USER 65532:65532
HEALTHCHECK CMD ["/usr/local/bin/martin", "health"]
ENTRYPOINT ["/usr/local/bin/martin"]

FROM scratch AS static
ARG TARGETPLATFORM
LABEL org.opencontainers.image.source="https://github.com/maplibre/martin"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"
LABEL org.opencontainers.image.documentation="https://maplibre.org/martin/"
LABEL org.opencontainers.image.vendor="maplibre"
LABEL org.opencontainers.image.authors="Yuri Astrakhan, Stepan Kuzmin and MapLibre contributors"
COPY --from=rootfs-static /rootfs/ /
COPY target_releases/$TARGETPLATFORM/* /usr/local/bin/
USER 65532:65532
HEALTHCHECK CMD ["/usr/local/bin/martin", "health"]
ENTRYPOINT ["/usr/local/bin/martin"]

FROM scratch AS glibc
ARG TARGETPLATFORM
LABEL org.opencontainers.image.source="https://github.com/maplibre/martin"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"
LABEL org.opencontainers.image.documentation="https://maplibre.org/martin/"
LABEL org.opencontainers.image.vendor="maplibre"
LABEL org.opencontainers.image.authors="Yuri Astrakhan, Stepan Kuzmin and MapLibre contributors"
COPY --from=rootfs-glibc /rootfs/ /
COPY target_releases/$TARGETPLATFORM/* /usr/local/bin/
USER 65532:65532
HEALTHCHECK CMD ["/usr/local/bin/martin", "health"]
ENTRYPOINT ["/usr/local/bin/martin"]
