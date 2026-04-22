import { Component } from '@angular/core';
import { NavItemComponent } from '../nav-item/nav-item.component';

export interface NavEntry {
  number: string;
  label: string;
  route: string;
}

// Hier alle Services eintragen — NavItemComponent rendert den Rest automatisch
const NAV_ENTRIES: NavEntry[] = [
  { number: '01', label: 'Key-Value Store',       route: '/key-value-store' },
  { number: '02', label: 'Age Verification',       route: '/age-verification' },
  { number: '03', label: 'Voting / Polling',        route: '/voting' },
  { number: '04', label: 'Sealed-Bid Auction',      route: '/auction' },
  { number: '05', label: 'Statistics Service',      route: '/statistics' },
  { number: '06', label: 'Genomics',                route: '/genomics' },
  { number: '07', label: 'Image Processing',        route: '/image-processing' },
  { number: '08', label: 'Leaderboard',             route: '/leaderboard' },
  { number: '09', label: 'Program Execution',       route: '/program-execution' },
];

@Component({
  selector: 'app-sidebar',
  imports: [NavItemComponent],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.css',
})
export class SidebarComponent {
  readonly entries = NAV_ENTRIES;
}
