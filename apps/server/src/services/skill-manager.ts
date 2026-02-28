import { readdirSync, mkdirSync, symlinkSync, existsSync } from 'node:fs';
import { join, basename, extname } from 'node:path';

export interface SkillInfo {
  name: string;
  path: string;
}

export class SkillManager {
  constructor(private skillsDir: string) {}

  listSkills(): SkillInfo[] {
    try {
      const files = readdirSync(this.skillsDir);
      return files
        .filter(f => extname(f) === '.md')
        .map(f => ({
          name: basename(f, '.md'),
          path: join(this.skillsDir, f),
        }));
    } catch {
      return [];
    }
  }

  async installSkills(worktreePath: string): Promise<void> {
    const targetDir = join(worktreePath, '.claude', 'skills');
    mkdirSync(targetDir, { recursive: true });

    const skills = this.listSkills();
    for (const skill of skills) {
      const targetPath = join(targetDir, `${skill.name}.md`);
      if (!existsSync(targetPath)) {
        symlinkSync(skill.path, targetPath);
      }
    }
  }
}
