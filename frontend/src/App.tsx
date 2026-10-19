import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";
import { UNAUTHORIZED_EVENT } from "./api/client";
import { keys, useAuth } from "./api/hooks";
import { Layout } from "./components/Layout";
import { Alert, LoadingBlock } from "./components/ui";
import { useLiveEvents } from "./lib/live";
import { DashboardPage } from "./pages/Dashboard";
import { DestinationsPage } from "./pages/Destinations";
import { InstanceDetailPage } from "./pages/InstanceDetail";
import { InstancesPage } from "./pages/Instances";
import { JobsPage } from "./pages/Jobs";
import { LoginPage } from "./pages/Login";
import { RunDetailPage } from "./pages/RunDetail";
import { RunsPage } from "./pages/Runs";
import { SettingsPage } from "./pages/Settings";

function AuthenticatedApp() {
  useLiveEvents(true);
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/instances" element={<InstancesPage />} />
        <Route path="/instances/:id" element={<InstanceDetailPage />} />
        <Route path="/jobs" element={<JobsPage />} />
        <Route path="/runs" element={<RunsPage />} />
        <Route path="/runs/:id" element={<RunDetailPage />} />
        <Route path="/destinations" element={<DestinationsPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}

export function App() {
  const { data: auth, isLoading, error } = useAuth();
  const queryClient = useQueryClient();

  useEffect(() => {
    const onUnauthorized = () => queryClient.invalidateQueries({ queryKey: keys.auth });
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, [queryClient]);

  if (isLoading) return <LoadingBlock />;
  if (error || !auth) {
    return (
      <div className="mx-auto max-w-md p-8">
        <Alert tone="red" title="The server is not reachable">
          {error instanceof Error ? error.message : "Unknown error"}
        </Alert>
      </div>
    );
  }

  return (
    <BrowserRouter>
      {auth.user ? <AuthenticatedApp /> : <LoginPage setup={auth.setup_required} />}
    </BrowserRouter>
  );
}
