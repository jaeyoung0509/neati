import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PageHeader from '../lib/components/PageHeader.svelte';
import { DEFAULT_DASHBOARD_TABS, dashboardStage } from '../lib/utils/dashboardNavigation';

describe('v1 core and beta destinations', () => {
  it('keeps storage, overview, performance and Keep Awake as the v1 core', () => {
    for (const tab of ['overview', 'storage', 'disk', 'performance', 'memory', 'awake', 'large-files', 'applications', 'developer-artifacts', 'settings']) {
      expect(dashboardStage(tab), tab).toBe('core');
    }
  });

  it('labels tool observers as beta without removing them from the default sidebar', () => {
    const beta = ['projects', 'usage', 'ai_control', 'docker', 'models', 'development_servers'];
    for (const tab of beta) expect(dashboardStage(tab), tab).toBe('beta');
    // Beta is a label, not a visibility rule: defaults still list them.
    for (const tab of ['projects', 'docker', 'models', 'development_servers']) {
      expect(DEFAULT_DASHBOARD_TABS).toContain(tab);
    }
  });

  it('renders one Beta badge in a beta page header and none otherwise', () => {
    const beta = render(PageHeader, { props: { title: 'Containers', stage: 'beta' } }).body;
    expect(beta.match(/>Beta</g)?.length).toBe(1);
    expect(beta).toContain('may be incomplete');
    expect(render(PageHeader, { props: { title: 'Cleanup' } }).body).not.toContain('>Beta<');
  });
});
