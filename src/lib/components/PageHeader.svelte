<script lang="ts">
  import type { Snippet } from "svelte";
  import Badge from "./Badge.svelte";

  interface Props {
    title: string;
    subtitle?: string;
    icon?: any;
    badge?:
      | string
      | {
          label: string;
          variant?: "default" | "secondary" | "outline" | "success" | "warning" | "ai" | "destructive";
        }
      | Snippet;
    class?: string;
    actions?: Snippet;
  }

  let {
    title,
    subtitle,
    icon: Icon,
    badge,
    class: className = "",
    actions,
  }: Props = $props();
</script>

<header class="page-header flex flex-col @2xl:flex-row @2xl:items-center justify-between gap-4 pb-4 border-b border-border {className}">
  <div class="flex items-center gap-3 min-w-0">
    {#if Icon}
      <div class="h-9 w-9 rounded-xl metric-icon-surface text-primary flex items-center justify-center shrink-0">
        <Icon size={18} strokeWidth={1.75} aria-hidden="true" />
      </div>
    {/if}
    <div class="min-w-0">
      <div class="flex flex-wrap items-center gap-2">
        <h1 class="text-title font-semibold text-foreground tracking-tight break-words">{title}</h1>
        {#if typeof badge === "string"}
          <Badge variant="outline">{badge}</Badge>
        {:else if typeof badge === "function"}
          {@render (badge as Snippet)()}
        {:else if badge && "label" in badge}
          <Badge variant={badge.variant ?? "outline"}>{badge.label}</Badge>
        {/if}
      </div>
      {#if subtitle}
        <p class="mt-1 max-w-prose text-body text-muted-foreground break-words">{subtitle}</p>
      {/if}
    </div>
  </div>

  {#if actions}
    <div class="flex flex-wrap items-center gap-2 @2xl:shrink-0">
      {@render actions()}
    </div>
  {/if}
</header>
