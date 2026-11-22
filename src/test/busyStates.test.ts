import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import DevelopmentServersView from '../routes/dashboard/DevelopmentServersView.svelte';
import DockerView from '../routes/dashboard/DockerView.svelte';
import { developmentPortsStore } from '../lib/stores/developmentPorts.svelte';
import { dockerStore } from '../lib/stores/docker.svelte';

// An empty result must not read as final before the first answer (#491).
describe('first-load states', () => {
  afterEach(() => {
    developmentPortsStore.hasLoaded = false;
    developmentPortsStore.listeners = [];
    dockerStore.status = null;
    dockerStore.error = null;
  });

  it('shows Dev Servers as checking until the first listener answer', () => {
    developmentPortsStore.hasLoaded = false;
    const pending = render(DevelopmentServersView).body;
    expect(pending).toContain('Checking listening ports…');
    expect(pending).not.toContain('No supported development or testing tools are listening.');

    developmentPortsStore.hasLoaded = true;
    const empty = render(DevelopmentServersView).body;
    expect(empty).toContain('No supported development or testing tools are listening.');
    expect(empty).not.toContain('Checking listening ports…');
  });

  it('shows Containers as checking until the Docker probe answers', () => {
    dockerStore.status = null;
    const pending = render(DockerView).body;
    expect(pending).toContain('Checking Docker…');
    expect(pending).not.toContain('Not Installed');
    expect(pending).not.toContain('Docker Daemon is Inactive');
  });
});
