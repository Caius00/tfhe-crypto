import { Routes } from '@angular/router';
import { VotingHomeComponent } from './home/voting-home.component';
import { CreateSessionComponent } from './create/create-session.component';
import { ManageSessionComponent } from './manage/manage-session.component';
import { JoinSessionComponent } from './join/join-session.component';
import { WaitingComponent } from './waiting/waiting.component';
import { VoteComponent } from './vote/vote.component';

export const VOTING_ROUTES: Routes = [
  { path: '', component: VotingHomeComponent },
  { path: 'create', component: CreateSessionComponent },
  { path: 'manage/:sessionId', component: ManageSessionComponent },
  { path: 'join', component: JoinSessionComponent },
  { path: 'waiting/:sessionId', component: WaitingComponent },
  { path: 'vote/:sessionId', component: VoteComponent },
];