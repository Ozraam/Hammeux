//! Villages : bâtiments, champs, réserves.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HouseState {
    Building,
    Inhabited,
    Abandoned,
    Ruin,
    Overgrown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct House {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub state: HouseState,
    /// Matériaux encore attendus par le chantier.
    pub wood_needed: u32,
    pub stone_needed: u32,
    /// Matériaux apportés.
    pub wood_in: u32,
    pub stone_in: u32,
    pub residents: Vec<u32>,
    pub family: String,
    /// Chantier réservé à un habitant qui veut sa maison.
    pub reserved_for: Option<u32>,
    /// Tick du dernier changement d'état (déclin).
    pub since: u64,
    /// Variante de couleur de toit.
    pub roof: u8,
}

impl House {
    /// Avancement du chantier, 0–1.
    pub fn progress(&self) -> f32 {
        let total = self.wood_in + self.stone_in + self.wood_needed + self.stone_needed;
        if total == 0 {
            1.0
        } else {
            (self.wood_in + self.stone_in) as f32 / total as f32
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldState {
    /// En friche (hiver, ou pas encore travaillé).
    Fallow,
    Ploughed,
    Sown,
    Ripe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub state: FieldState,
    /// Avancement du travail en cours (labour, semis, moisson), 0–1.
    pub work: f32,
    /// Pousse depuis le semis, 0–1.
    pub growth: f32,
    /// Fertilité moyenne des cases, 0–1.
    pub fertility: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stock {
    pub food: f32,
    pub wood: u32,
    pub stone: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Village {
    pub id: u32,
    pub name: String,
    /// Grenier (2 × 2 cases, coin haut-gauche) ; sa porte est la case sous le coin gauche.
    pub x: u32,
    pub y: u32,
    pub well: (u32, u32),
    pub color: u32,
    pub founded: u64,
    pub houses: Vec<House>,
    pub fields: Vec<Field>,
    pub stock: Stock,
    pub hungry_days: u32,
    /// Une carrière est-elle accessible ? Sinon les maisons sont tout en bois.
    pub has_stone: bool,
}

impl Village {
    /// Case devant le grenier, où l'on dépose et prend.
    pub fn door(&self) -> (u32, u32) {
        (self.x, self.y + 2)
    }

    pub fn house(&self, id: u32) -> Option<&House> {
        self.houses.iter().find(|h| h.id == id)
    }

    pub fn house_mut(&mut self, id: u32) -> Option<&mut House> {
        self.houses.iter_mut().find(|h| h.id == id)
    }

    pub fn field_mut(&mut self, id: u32) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.id == id)
    }
}
