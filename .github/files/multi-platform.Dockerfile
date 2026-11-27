# Targets: full, static (legacycpu), glibc (default). Wolfi packages on scratch: no shell, but scanners see the apk db.

# Runs on the build platform and installs the target's packages, so arm64 builds need no emulation.
FROM --platform=$BUILDPLATFORM cgr.dev/chainguard/wolfi-base:latest@sha256:4cf5932a0f95a88af352f77e4efa67480cab0e1f7a48f0eddca3cce003a7c0fb AS wolfi
ARG TARGETARCH
# wolfi-base defaults to Chainguard's repository. No scripts: the new root has no shell to run them.
ENV APK="apk add --no-cache --no-scripts --initdb --root /rootfs --keys-dir /etc/apk/keys --repository https://packages.wolfi.dev/os --arch"
# `nonroot`, the uid and gid that Chainguard and distroless images run as.
RUN mkdir -p /rootfs/etc /rootfs/home/nonroot \
    && chown 65532:65532 /rootfs/home/nonroot \
    && echo "$TARGETARCH" | sed 's/^amd64$/x86_64/; s/^arm64$/aarch64/' > /apk-arch

FROM wolfi AS rootfs-static
RUN $APK "$(cat /apk-arch)" wolfi-baselayout ca-certificates-bundle tzdata \
    && echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /rootfs/etc/passwd \
    && echo 'nonroot:x:65532:' >> /rootfs/etc/group

FROM wolfi AS rootfs-glibc
RUN $APK "$(cat /apk-arch)" wolfi-baselayout ca-certificates-bundle tzdata glibc libgcc \
    && echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /rootfs/etc/passwd \
    && echo 'nonroot:x:65532:' >> /rootfs/etc/group

FROM wolfi AS rootfs-full
# Rendering uses llvmpipe via libEGL.so.1; with no Vulkan loader, the Vulkan drivers never load.
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
