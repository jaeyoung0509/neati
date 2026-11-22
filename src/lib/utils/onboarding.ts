export interface OnboardingStep {
  id: 'what' | 'safety' | 'access';
  title: string;
  paragraphs: string[];
  points?: string[];
}

/** First-run guide copy, kept in one place so the dialog stays presentation only. */
export const ONBOARDING_STEPS: readonly OnboardingStep[] = [
  {
    id: 'what',
    title: 'Welcome to neati',
    paragraphs: [
      'neati finds storage that developer and AI tools leave behind: package and compiler caches, AI assistant caches and logs, container data, and build output.',
      'Scan Storage measures what is there. Nothing is removed until you choose Clean.',
    ],
  },
  {
    id: 'safety',
    title: 'Cleaning is permanent',
    paragraphs: [
      'Items you clean are removed permanently. They do not go to the Trash and cannot be restored by neati.',
    ],
    points: [
      'Caches marked Rebuild are downloaded or rebuilt by their tool the next time it needs them.',
      'Source code, documents, credentials and settings stay outside what neati can clean.',
      'Caches in use by a running app are skipped.',
    ],
  },
  {
    id: 'access',
    title: 'Optional setup',
    paragraphs: [
      'Both settings are optional and can be changed later in Settings.',
    ],
  },
];
