import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Shell } from './components/shell/Shell';
import { Toaster } from './components/ui/toast';
import { StudioProvider } from './context/Studio';
import { applyCachedTheme } from './lib/theme';
import './index.css';

// Before first paint, so a light-theme user never sees a dark flash.
applyCachedTheme();

const client = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={client}>
        <StudioProvider>
          <Shell />
          <Toaster />
        </StudioProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
}
