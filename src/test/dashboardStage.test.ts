import { describe, expect, it } from 'vitest';
import { SECTIONS, dashboardStage } from '../lib/utils/dashboardNavigation';

describe('v1 core and beta destinations', () => {
  it('keeps storage, overview, performance and Keep Awake as the v1 core', () => {
    for (const tab of ['overview', 'storage', 'disk', 'performance', 'memory', 'awake', 'large-files', 'applications', 'developer-artifacts', 'settings']) {
      expect(dashboardStage(tab), tab).toBe('core');
    }
  });

  it('labels tool observers as beta without removing them from navigation', () => {
    const beta = ['projects', 'usage', 'ai_control', 'docker', 'models', 'development_servers'];
    for (const tab of beta) expect(dashboardStage(tab), tab).toBe('beta');
    // Beta is a label, not a visibility rule: every beta view is still a tab.
    const tabs = SECTIONS.flatMap((section) => section.routes.map((entry) => entry.route));
    for (const tab of ['projects', 'ai_control', 'docker', 'models', 'development_servers']) {
      expect(tabs).toContain(tab);
    }
  });
});
