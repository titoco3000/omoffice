import { useEffect, useState } from 'react';
import { useAuth } from '../auth/AuthContext';
import { useActiveMeeting } from '../meeting/MeetingContext';
import { Link, usePath } from '../router';
import Avatar from '../ui/Avatar';
import Button from '../ui/Button';
import {
  IconChevronLeft,
  IconChevronRight,
  IconList,
  IconLogo,
  IconMeeting,
} from '../ui/icons';
import OrgUserList from './OrgUserList';

const COLLAPSED_KEY = 'omoffice_sidebar_collapsed';

/**
 * The frame every page but `/` and `/login` sits in.
 *
 * Desktop is a sidebar plus the rest of the screen; a phone gets a bottom
 * bar instead and the roster moves to its own page (`/users`), which is
 * what the bar's third slot links to when no meeting is running.
 */
export default function NavContainer({ children }) {
  const { email } = useAuth();
  const { code } = useActiveMeeting();
  const path = usePath();
  const [collapsed, setCollapsed] = useState(loadCollapsed);

  useEffect(() => {
    try {
      localStorage.setItem(COLLAPSED_KEY, collapsed ? '1' : '0');
    } catch {
      // The preference just won't survive a refresh.
    }
  }, [collapsed]);

  const onMeetingPage = path.startsWith('/meeting/');

  return (
    <div className='flex h-dvh w-full overflow-hidden'>
      <aside
        className={`hidden shrink-0 flex-col border-r border-line bg-surface md:flex ${
          collapsed ? 'w-rail' : 'w-side'
        }`}
      >
        <div
          className={`flex gap-2 px-3 py-3 ${
            collapsed ? 'flex-col items-center' : 'items-center'
          }`}
        >
          <IconLogo className='h-7 w-7 shrink-0 text-brand' />
          {!collapsed && (
            <span className='min-w-0 flex-1 truncate text-sm font-semibold'>
              omoffice
            </span>
          )}
          <Button
            variant='ghost'
            size='icon-sm'
            title={collapsed ? 'expand sidebar' : 'collapse sidebar'}
            aria-label={collapsed ? 'expand sidebar' : 'collapse sidebar'}
            onClick={() => setCollapsed((current) => !current)}
          >
            {collapsed ? (
              <IconChevronRight className='h-4 w-4' />
            ) : (
              <IconChevronLeft className='h-4 w-4' />
            )}
          </Button>
        </div>

        <div className='flex min-h-0 flex-1 flex-col justify-end overflow-hidden px-2 pb-2'>
          <OrgUserList compact={collapsed} />
        </div>

        <Link
          to='/me'
          title={email ?? 'profile'}
          className={`flex items-center gap-2 border-t border-line px-3 py-3 transition-colors hover:bg-muted ${
            collapsed ? 'justify-center' : ''
          }`}
        >
          <Avatar email={email} size='sm' />
          {!collapsed && (
            <span className='min-w-0 flex-1 truncate text-xs text-ink-muted'>
              {email}
            </span>
          )}
        </Link>
      </aside>

      <main className='flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden pb-16 md:pb-0'>
        {children}
      </main>

      <nav className='fixed inset-x-0 bottom-0 z-40 flex h-16 items-center justify-between border-t border-line bg-surface px-6 md:hidden'>
        <Link to='/me' aria-label='profile' className='p-2'>
          <Avatar email={email} size='sm' />
        </Link>
        <IconLogo className='h-7 w-7 text-brand' />
        {code && !onMeetingPage ? (
          <Link
            to={`/meeting/${encodeURIComponent(code)}`}
            aria-label='back to the meeting'
            className='p-2 text-success'
          >
            <IconMeeting className='h-6 w-6' />
          </Link>
        ) : (
          <Link to='/users' aria-label='people' className='p-2 text-ink-muted'>
            <IconList className='h-6 w-6' />
          </Link>
        )}
      </nav>
    </div>
  );
}

function loadCollapsed() {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === '1';
  } catch {
    return false; // private browsing, storage disabled, etc.
  }
}
