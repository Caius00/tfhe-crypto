import { Routes } from '@angular/router';
import { ShellComponent } from './layout/shell/shell.component';

export const routes: Routes = [
  {
    path: '',
    component: ShellComponent,
    children: [
      {
        path: '',
        redirectTo: 'key-value-store',
        pathMatch: 'full',
      },
      {
        path: 'key-value-store',
        loadChildren: () =>
          import('./features/key-value-store/key-value-store.routes').then(
            (m) => m.KEY_VALUE_STORE_ROUTES
          ),
      },
      {
        path: 'age-verification',
        loadChildren: () =>
          import('./features/age-verification/age-verification.routes').then(
            (m) => m.AGE_VERIFICATION_ROUTES
          ),
      },
      {
        path: 'voting',
        loadChildren: () =>
          import('./features/voting/voting.routes').then(
            (m) => m.VOTING_ROUTES
          ),
      },
      {
        path: 'auction',
        loadChildren: () =>
          import('./features/auction/auction.routes').then(
            (m) => m.AUCTION_ROUTES
          ),
      },
      {
        path: 'statistics',
        loadChildren: () =>
          import('./features/statistics/statistics.routes').then(
            (m) => m.STATISTICS_ROUTES
          ),
      },
      {
        path: 'genomics',
        loadChildren: () =>
          import('./features/genomics/genomics.routes').then(
            (m) => m.GENOMICS_ROUTES
          ),
      },
      {
        path: 'image-processing',
        loadChildren: () =>
          import('./features/image-processing/image-processing.routes').then(
            (m) => m.IMAGE_PROCESSING_ROUTES
          ),
      },
      {
        path: 'leaderboard',
        loadChildren: () =>
          import('./features/leaderboard/leaderboard.routes').then(
            (m) => m.LEADERBOARD_ROUTES
          ),
      },
      {
        path: 'program-execution',
        loadChildren: () =>
          import('./features/program-execution/program-execution.routes').then(
            (m) => m.PROGRAM_EXECUTION_ROUTES
          ),
      },
    ],
  },
];
