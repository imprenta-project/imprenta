import { Contrast, MonitorCog, Moon, Sun } from 'lucide-react';
import { createContext, use, useEffect, useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

/**
 * Ink or paper.
 *
 * Both modes are real in this brand, which is not the usual courtesy to people
 * who dislike dark UIs: Imprenta's output is a printed sheet, so paper is the
 * medium and not a preference. Ink is still the default, because this window
 * sits open beside an editor and because a page only reads as a page when what
 * surrounds it is not one.
 *
 * The mode is a class on `<html>`, which is what Tailwind's `dark:` variant
 * looks for, and it is also applied by a script in `index.html` before React
 * mounts. Without that the first frame is paper and the second is ink, which
 * is the flash every theme switcher is judged by.
 */
export type Theme = 'ink' | 'paper' | 'system';

const THEMES: Theme[] = ['ink', 'paper', 'system'];

/** Namespaced, because this runs on `localhost:4321` alongside everything else
    somebody has ever served from `localhost:4321`. */
const KEY = 'imprenta.theme';

const ThemeContext = createContext<{ theme: Theme; setTheme(theme: Theme): void }>({
  theme: 'ink',
  setTheme: () => {},
});

export function useTheme() {
  return use(ThemeContext);
}

/** A stored value only counts if it is still one of the modes. */
function stored(): Theme {
  try {
    const found = localStorage.getItem(KEY);
    return THEMES.includes(found as Theme) ? (found as Theme) : 'ink';
  } catch {
    // Storage can be denied outright, and a preview that will not start
    // because of a browser setting is worse than one that forgets.
    return 'ink';
  }
}

export function ThemeProvider({ children }: { children?: React.ReactNode }) {
  const [theme, setTheme] = useState<Theme>(stored);

  useEffect(() => {
    const dark = () =>
      theme === 'system'
        ? window.matchMedia('(prefers-color-scheme: dark)').matches
        : theme === 'ink';

    const apply = () => document.documentElement.classList.toggle('dark', dark());
    apply();

    try {
      localStorage.setItem(KEY, theme);
    } catch {
      // As above: remembering is a nicety, working is not.
    }

    if (theme !== 'system') {
      return;
    }
    // Only worth listening to while following the system: otherwise the
    // author has said what they want and the OS does not get a vote.
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  }, [theme]);

  const value = useMemo(() => ({ theme, setTheme }), [theme]);
  return <ThemeContext value={value}>{children}</ThemeContext>;
}

const ICONS = { ink: Moon, paper: Sun, system: MonitorCog } as const;

const LABELS = {
  ink: 'Ink',
  paper: 'Paper',
  system: 'Follow the system',
} as const;

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  const Icon = ICONS[theme] ?? Contrast;

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              render={
                <Button variant="ghost" size="icon-sm" aria-label="Theme">
                  <Icon />
                </Button>
              }
            />
          }
        />
        <TooltipContent side="bottom">Theme</TooltipContent>
      </Tooltip>

      <DropdownMenuContent align="end" className="min-w-40">
        <DropdownMenuRadioGroup value={theme} onValueChange={(next) => setTheme(next as Theme)}>
          {THEMES.map((each) => {
            const Each = ICONS[each];
            return (
              <DropdownMenuRadioItem key={each} value={each}>
                <Each className="text-muted-foreground" />
                {LABELS[each]}
              </DropdownMenuRadioItem>
            );
          })}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
