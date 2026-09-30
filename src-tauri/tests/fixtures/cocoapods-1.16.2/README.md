This MIT-licensed fixture is the unmodified CocoaPods 1.16.2
[`cache/clean.rb`](https://github.com/CocoaPods/CocoaPods/blob/1.16.2/lib/cocoapods/command/cache/clean.rb)
(source Git blob `aa3563faff09db6b7971dc4cbd4dc714be61b4b9`).
The native macOS Rust regression runs its actual `clear_cache` method through
system Ruby in a disposable RubyGems test facade. It proves whole-root removal
and preservation of sibling repositories, project Pods, configuration and
credentials. The facade is not an installed CocoaPods distribution; plugin
resolution, CocoaPods startup and third-party Ruby installations still need
an installed-distribution validation run.
