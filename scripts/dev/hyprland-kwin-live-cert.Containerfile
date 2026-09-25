FROM fedora@sha256:a9bab18d01cf2c2cf62f3e79c72623405bce14ac062995cc6651a3073c802e41

RUN printf '%s\n' \
      '[semwright-hyprland]' \
      'name=Semwright pinned Hyprland COPR runtime' \
      'baseurl=https://download.copr.fedorainfracloud.org/results/nett00n/hyprland/fedora-45-x86_64/' \
      'enabled=1' \
      'gpgcheck=1' \
      'repo_gpgcheck=0' \
      'gpgkey=https://download.copr.fedorainfracloud.org/results/nett00n/hyprland/pubkey.gpg' \
      > /etc/yum.repos.d/semwright-hyprland.repo \
    && dnf -y --setopt=install_weak_deps=False install \
      hyprland-0.56.2-17.fc45.x86_64 \
      kwin-6.7.5-1.fc46.x86_64 \
      foot-1.28.0-1.fc46.x86_64 \
      mesa-dri-drivers-26.2.3-1.fc46.x86_64 \
      libdrm-2.4.134-2.fc45.x86_64 \
      wayland-utils-1.3.0-4.fc45.x86_64 \
      dbus-daemon-1.16.2-6.fc45.x86_64 \
      libcap-2.78-2.fc45.x86_64 \
      util-linux-2.42.4-2.fc46.x86_64 \
    && (setcap -r /usr/bin/kwin_wayland || true) \
    && dnf clean all \
    && useradd -u 1000 -M semwright \
    && mkdir -p /run/semwright /home/semwright \
    && chown -R 1000:1000 /run/semwright /home/semwright

USER 1000:1000
ENV HOME=/home/semwright
ENV XDG_RUNTIME_DIR=/run/semwright
CMD ["/bin/bash"]
