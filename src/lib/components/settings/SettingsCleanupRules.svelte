<script lang="ts">
  import { settingsStore } from '../../stores/settings.svelte';
  import { projectCleanupStore } from '../../stores/projectCleanup.svelte';
  import { resolveCleanupRules, type ResolvedCleanupRules } from '../../utils/cleanupRules';
  import Card from '../Card.svelte';
  import Switch from '../Switch.svelte';

  /**
   * The person's rules for project leftovers in one-button Clean (#504).
   * The backend owns the decision; this only edits the saved rules.
   */
  const BUILD_OUTPUT_DAYS = [0, 3, 7, 14, 30, 60, 90];
  const DEPENDENCY_DAYS = [7, 14, 30, 60, 90, 180];
  const WORKTREE_DAYS = [1, 3, 7, 14, 30, 60];

  let rules = $derived<ResolvedCleanupRules>(resolveCleanupRules(settingsStore.settings.cleanup_rules));

  async function save(next: ResolvedCleanupRules) {
    // Inclusion is decided at scan time, so a saved rule asks for a fresh
    // project check; the backend also refuses to reuse the old decision.
    if (await settingsStore.save({ cleanup_rules: next })) {
      void projectCleanupStore.refresh();
    }
  }

  function buildOutputLabel(days: number): string {
    return days === 0 ? 'Whenever nothing has it open' : `After ${days} days without work`;
  }
</script>

<div class="space-y-3">
  <div>
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Project cleanup rules
    </h3>
    <p class="text-meta text-muted-foreground mt-1">
      Applies to project folders you added. Clean permanently removes what a rule includes, and the
      project rebuilds or reinstalls it when needed. Anything a process has open is always left alone.
    </p>
  </div>
  <Card class="p-4 space-y-4 bg-card divide-y divide-border/60">
    <div class="flex items-start justify-between gap-5 text-meta">
      <div class="min-w-0">
        <label for="cleanup-build-output-days" class="font-medium text-foreground">Build output</label>
        <div class="text-meta text-muted-foreground mt-0.5">
          Cargo target, Gradle, Maven, CMake, Swift, Zig, .NET obj and similar compiler output.
        </div>
      </div>
      <select
        id="cleanup-build-output-days"
        value={String(rules.build_output_min_idle_days)}
        onchange={(event) =>
          save({ ...rules, build_output_min_idle_days: Number((event.target as HTMLSelectElement).value) })}
        class="h-8 shrink-0 rounded-md border border-border-strong bg-card px-2 text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {#each BUILD_OUTPUT_DAYS as days (days)}
          <option value={String(days)}>{buildOutputLabel(days)}</option>
        {/each}
      </select>
    </div>

    <div class="flex items-start justify-between gap-5 text-meta pt-3">
      <div class="min-w-0">
        <div class="font-medium text-foreground">Dependency folders of inactive projects</div>
        <div class="text-meta text-muted-foreground mt-0.5">
          node_modules, Python environments, vendor, bundles and deps. Only when the project's lockfile
          sits beside them, so a reinstall reproduces them. Reinstalling downloads them again.
        </div>
        {#if rules.dependency_stores.enabled}
          <div class="mt-2 flex items-center gap-2">
            <label for="cleanup-dependency-days" class="text-muted-foreground">Include after</label>
            <select
              id="cleanup-dependency-days"
              value={String(rules.dependency_stores.idle_days)}
              onchange={(event) =>
                save({
                  ...rules,
                  dependency_stores: {
                    ...rules.dependency_stores,
                    idle_days: Number((event.target as HTMLSelectElement).value),
                  },
                })}
              class="h-8 rounded-md border border-border-strong bg-card px-2 text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {#each DEPENDENCY_DAYS as days (days)}
                <option value={String(days)}>{days} days without work</option>
              {/each}
            </select>
          </div>
        {/if}
      </div>
      <Switch
        checked={rules.dependency_stores.enabled}
        onchange={(enabled) => save({ ...rules, dependency_stores: { ...rules.dependency_stores, enabled } })}
        ariaLabel="Dependency folders of inactive projects"
      />
    </div>

    <div class="flex items-start justify-between gap-5 text-meta pt-3">
      <div class="min-w-0">
        <div class="font-medium text-foreground">Finished Git worktrees</div>
        <div class="text-meta text-muted-foreground mt-0.5">
          Checkouts whose work is committed and on a branch, that nothing uses. Git removes them; branches and commits
          stay. Worktrees with uncommitted or untracked files, locks or submodules are never included.
        </div>
        <div class="mt-2 flex items-center gap-2">
          <label for="cleanup-worktree-days" class="text-muted-foreground">Finished after</label>
          <select
            id="cleanup-worktree-days"
            value={String(rules.worktrees.idle_days)}
            onchange={(event) =>
              save({
                ...rules,
                worktrees: { ...rules.worktrees, idle_days: Number((event.target as HTMLSelectElement).value) },
              })}
            class="h-8 rounded-md border border-border-strong bg-card px-2 text-meta text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {#each WORKTREE_DAYS as days (days)}
              <option value={String(days)}>{days} {days === 1 ? 'day' : 'days'} without work</option>
            {/each}
          </select>
        </div>
      </div>
      <Switch
        checked={rules.worktrees.enabled}
        onchange={(enabled) => save({ ...rules, worktrees: { ...rules.worktrees, enabled } })}
        ariaLabel="Finished Git worktrees"
      />
    </div>
  </Card>
</div>
