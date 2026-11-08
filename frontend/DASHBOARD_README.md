# VNSync Dashboard

A modern desktop application dashboard for managing visual novel games, built with Vue 3 and Nuxt UI.

## Features

### 🏠 Home Dashboard
- **Statistics Overview**: Total games, in-progress, completed, and total playtime
- **Recent Games**: Visual cards showing game thumbnails, progress, and status
- **Quick Actions**: Easy access to add games, settings, and view statistics

### ➕ Add Game
- **Comprehensive Form**: Game name, developer, publisher, release date
- **Game Details**: Genre, rating, status, and executable path
- **Visual Preview**: Thumbnail preview with placeholder support
- **Form Validation**: Required fields and proper input types

### ⚙️ Settings
- **General Settings**: Theme, language, startup preferences
- **Game Management**: Library path, auto-detection, default status
- **Notifications**: Customizable notification preferences
- **Sync & Backup**: Cloud sync settings and backup options

### ℹ️ About
- **App Information**: Version, developer, license details
- **Technology Stack**: Built with Vue 3, Nuxt UI, Tailwind CSS, Go, and Wails
- **Features Showcase**: Visual cards highlighting key capabilities
- **Contributors**: Developer information and links

### 👤 Account
- **Profile Management**: Name, email, bio, and avatar
- **Statistics**: Personal gaming statistics and achievements
- **Privacy Settings**: Control what information is public
- **Recent Activity**: Timeline of recent actions
- **Account Management**: Export data and account deletion options

## Navigation

The sidebar includes:
- **Home**: Main dashboard with overview
- **Add Game**: Form to add new games to the library
- **Settings**: Application configuration
- **About**: Application information and credits
- **Account**: User profile and preferences (with avatar in footer)

## Design System

Built using Nuxt UI v4 components:
- **Cards**: For content organization and visual hierarchy
- **Buttons**: Consistent styling with icons and variants
- **Forms**: Input fields, selects, checkboxes, and textareas
- **Badges**: Status indicators and labels
- **Progress**: Visual progress tracking
- **Icons**: Heroicons integration for consistent iconography

## Color Scheme

- **Primary**: Blue tones for main actions and branding
- **Success**: Green for completed items and positive states
- **Info**: Blue for informational content
- **Warning**: Orange for attention-grabbing elements
- **Error**: Red for errors and destructive actions
- **Neutral**: Gray for secondary content and disabled states

## Responsive Design

The dashboard is fully responsive with:
- **Mobile**: Single column layout with stacked cards
- **Tablet**: Two-column grid for optimal space usage
- **Desktop**: Three-column layout for maximum efficiency

## Dark Mode Support

Built-in support for light and dark themes with automatic system preference detection.

## Future Enhancements

- Game library management
- Progress tracking with save states
- Achievement system
- Cloud synchronization
- Import/export functionality
- Plugin system for game engines
