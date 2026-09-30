import { useEffect } from "react";
import { AuthProvider, useAuth } from "../core/auth/AuthContext";
import { ThemeProvider } from "../core/theme/ThemeContext";
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

const AUTHENTICATED_ROUTES = {
  "/in": InPage,
  "/profile": Profile,
  "/org": Org,
  "/org/new": OrgNew,
};

function RedirectTo({ to }) {
  useEffect(() => navigate(to), [to]);
  return null;
}

function AuthenticatedApp({ path }) {
  const Page = AUTHENTICATED_ROUTES[path];
  return (
    <OrgSessionProvider>
      <MapEditorProvider>
        <MeetingSessionProvider>
          <MapRelocationProvider>
            <MeetingLayoutProvider>
              <NavContainer>
                {Page ? <Page /> : <RedirectTo to="/in" />}
              </NavContainer>
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
