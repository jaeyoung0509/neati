import type {
  AgentSkillInstall,
  AgentSkillState,
  AgentSkillTarget,
  CommandLineStatus,
} from '../../models/types';

const EXECUTABLE = '/Applications/neati.app/Contents/MacOS/Neati';

/** Browser-preview fixture: `neati` is not linked yet; Claude has an old copy. */
export function createCommandLineMock() {
  const skills: Record<AgentSkillTarget, AgentSkillState> = { claude: 'outdated', codex: 'missing' };
  const pathOf = (target: AgentSkillTarget) =>
    `/Users/mock/.${target}/skills/neati/SKILL.md`;

  return {
    async status(): Promise<CommandLineStatus> {
      return {
        executable: EXECUTABLE,
        command_path: null,
        linked_here: false,
        link_command: `ln -sf "${EXECUTABLE}" /opt/homebrew/bin/neati`,
        skills: (Object.keys(skills) as AgentSkillTarget[]).map((target) => ({
          target,
          path: pathOf(target),
          state: skills[target],
        })),
      };
    },

    async install(target: AgentSkillTarget): Promise<AgentSkillInstall> {
      if (skills[target] === 'foreign') {
        throw new Error(`${pathOf(target)} already exists and was not written by neati.`);
      }
      const replaced = skills[target] !== 'missing';
      skills[target] = 'current';
      return { target, path: pathOf(target), replaced };
    },
  };
}
