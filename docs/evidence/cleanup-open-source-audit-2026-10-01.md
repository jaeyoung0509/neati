# Cleanup source comparison — October 1, 2026

The owner requested parallel macOS, Windows and Linux cleaner research against
neati `develop@70c0c62ac6681b1697c7fb428326cb6d6dd234b7`, version 0.3.93.
Three source audits inspected pinned revisions and compared actual neati
catalogs, platform adapters and safety boundaries. No cleaner was installed or
run, no real files were removed, and no native Windows/Linux runtime or new
speed/reclaimed-space benchmark was available.

## Actionable findings

| Issue | Confirmed gap | First bounded slice |
| --- | --- | --- |
| [#390](https://github.com/jaeyoung0509/neati/issues/390) | Developer Artifact recognition does not inspect authored descendants, tracked content or deployment keypairs before a whole-unit Trash move. | Fresh ownership/safety evidence at discovery, plan and mutation boundaries, with temporary Git/Anchor fixtures. This is a source guard gap, not a reproduced user-data loss. |
| [#391](https://github.com/jaeyoung0509/neati/issues/391) | Exact SvelteKit `.svelte-kit` and Next.js `.next` artifact rules are absent. | Verified default outputs in explicit workspaces; unknown wrappers stay traversable and mutation depends on #390 and owner-use proof. |
| [#392](https://github.com/jaeyoung0509/neati/issues/392) | Windows installed-app inspection is deliberately unavailable. | Bounded read-only registration inventory with native fixture tests; Store coverage is separately qualified and uninstallation remains unavailable. |
| [#393](https://github.com/jaeyoung0509/neati/issues/393) | Windows has no exact file-use observer equivalent to macOS. | Evaluate documented resource observations and completeness; current stopped-browser policy remains until native proof supports a narrow rule. |
| [#394](https://github.com/jaeyoung0509/neati/issues/394) | An empty `XDG_CACHE_HOME` omits named caches instead of using the documented default. | A pure v0.3.93 path probe reproduced empty → None, unset → default. Normalize exactly empty values without relaxing other invalid overrides. |
| [#395](https://github.com/jaeyoung0509/neati/issues/395) | Linux desktop features are intentionally unavailable; current Ubuntu exporters do not establish cleanup readiness. | Exact read-only freedesktop thumbnail inspection, native Linux fixtures and capability gating before any availability promotion. |

Broader browser layouts remain #382; removed-app review remains #383; named
tool owner operations remain #381. Measured performance remains #379, Windows
interactive acceptance #380 and native Quick Panel bounds #376. Initial UI work
#378 shipped in #384 / 0.3.93 and was closed with its remaining native/accessibility
acceptance transferred to #389; the new scan/motion request is #388. Xcode #350
stays deferred/open and CLI #335 closed. Steam inventory, duplicate inspection
and distro package-owner assessment are later source leads, not newly enabled
cleanup actions.

## Primary sources reviewed

- [Mole](https://github.com/tw93/Mole/tree/c430bac637929ebede043df81d3bef319428309c): authored-content guards and the separate `mo purge` artifact catalog. `mo purge` coverage is not `mo clean` reclaim evidence.
- [mac-cleanup-py](https://github.com/mac-cleanup/mac-cleanup-py/tree/ba4d6ef1e7d2795439f0d3aaf48f59e61b49f656): named macOS stores. State reset and process termination do not define neati's deletion authority.
- [BleachBit](https://github.com/bleachbit/bleachbit/tree/8e6a42edbdbe9b3294729f8ad9cd9615b8828f13): Windows/Linux namespaces, thumbnails and package-owner differences. History/registry reset and Explorer restart are outside neati's ordinary cleanup contract.
- [Bulk Crap Uninstaller](https://github.com/BCUninstaller/Bulk-Crap-Uninstaller/tree/30da609384c98ba6e35c6530129541ea4ff3970a): registration/Store discovery separation, without importing uninstall operations.
- [Czkawka](https://github.com/qarmin/czkawka/tree/eb8b91dbb4d25dff416202674ee0de84ee4b5e5c): staged duplicate-inspection lead only, outside the six immediate issues.
- [Flatpak](https://github.com/flatpak/flatpak/tree/acb9dc7959ad6eed865acb8fd1f2398095f54017) and [DNF5](https://github.com/rpm-software-management/dnf5/blob/02b803fec632d8ddaf1693752af448f8d90a1ac2/doc/dnf5.conf.5.rst): installation scope, shared/pinned refs and effective cache roots. They require dedicated owner contracts before future actions.
- [XDG Base Directory Specification 0.8](https://specifications.freedesktop.org/basedir/latest/), [thumbnail layout](https://specifications.freedesktop.org/thumbnail/latest/directory.html), [SvelteKit output](https://svelte.dev/docs/kit/project-structure#Other-files-svelte-kit), [Next.js distDir](https://nextjs.org/docs/app/api-reference/config/next-config-js/distDir) and linked Microsoft APIs in #392/#393 provide vendor contracts for independent implementation.

[Stacer](https://github.com/oguzhaninan/Stacer/blob/a44d0565a05c996b1058f950f1d308e1964c8320/README.md)
explicitly announces abandonment and is historical comparison only.
[Pearcleaner](https://github.com/alienator88/Pearcleaner/blob/7724df7111bff82ae243301cf701992ef05ecf19/LICENSE.md)
is source-available under Apache 2.0 plus Commons Clause; it is not labelled
open-source in this audit. Its absent-owner/undo designs are contextual leads,
not imported code or new authority. No upstream cleaner implementation was
copied or translated.

Each created issue contains pinned neati/upstream file evidence, concrete
acceptance criteria, safety limits and platform-specific proof requirements.
Larger source catalogs alone do not prove missing reclaimable gigabytes:
observed stores, overlapping bytes, protected state, owner operations, Trash
movement and free-space changes remain distinct populations.
