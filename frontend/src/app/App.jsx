import { useEffect } from "react";
import { AuthProvider, useAuth } from "../core/auth/AuthContext";
import { ThemeProvider } from "../core/theme/ThemeContext";
import DebugPanel from "../core/ui/DebugPanel";
import { RouterProvider, useLocation, navigate } from "../core/Router";
import {
  MapEditorProvider,
  MapModeProvider,
  MapRelocationProvider,
} from "../features/map";
import { OrgSessionProvider } from "../features/org";
import {
  MeetingSessionProvider,
  MeetingLayoutProvider,
} from "../features/meetings";
import NavContainer from "./NavContainer";
import Home from "../pages/Home";
import Login from "../pages/Login";
import InPage from "../pages/InPage";
import Profile from "../pages/Profile";
import Org from "../pages/Org";
import OrgNew from "../pages/OrgNew";
import Admin from "../pages/Admin";
import AdminOrg from "../pages/AdminOrg";
import DevCosts from "../pages/DevCosts";

const AUTHENTICATED_ROUTES = {
  "/in": InPage,
  "/profile": Profile,
  "/org": Org,
  "/org/new": OrgNew,
  "/admin": Admin,
  "/dev-costs": DevCosts,
};

const ADMIN_ORG_PATH = /^\/admin\/org\/([^/]+)$/;

// A path's page: an exact route, or `/admin/org/<id>` (the one route with a
// parameter -- `core/Router` matches whole paths only).
function pageFor(path) {
  const Page = AUTHENTICATED_ROUTES[path];
  if (Page) return <Page />;
  const orgId = path.match(ADMIN_ORG_PATH)?.[1];
  if (orgId) return <AdminOrg orgId={orgId} />;
  return <RedirectTo to="/in" />;
}

function RedirectTo({ to }) {
  useEffect(() => navigate(to), [to]);
  return null;
}

function AuthenticatedApp({ path }) {
  return (
    <OrgSessionProvider>
      <MapEditorProvider>
        <MeetingSessionProvider>
          <MapRelocationProvider>
            <MeetingLayoutProvider>
              <NavContainer>{pageFor(path)}</NavContainer>
            </MeetingLayoutProvider>
          </MapRelocationProvider>
        </MeetingSessionProvider>
      </MapEditorProvider>
    </OrgSessionProvider>
  );
}

function Pages() {
  const path = useLocation();
  const { token } = useAuth();

  if (path === "/") return <Home />;
  if (path === "/login") return token ? <RedirectTo to="/in" /> : <Login />;
  if (!token) return <Login />;
  return <AuthenticatedApp path={path} />;
}

export default function App() {
  return (
    <ThemeProvider>
      <DebugPanel />
      <MapModeProvider>
        <AuthProvider>
          <RouterProvider>
            <Pages />
          </RouterProvider>
        </AuthProvider>
      </MapModeProvider>
    </ThemeProvider>
  );
}
