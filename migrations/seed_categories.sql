-- AgriMarket category seed
-- Run via: docker exec -i agrimarket_postgres psql -U agrimarket -d agrimarket

INSERT INTO categories (name, slug, description) VALUES
  ('Poultry',         'poultry',         'Chicken, turkey, duck and other poultry'),
  ('Dairy',           'dairy',           'Milk, cheese, butter, cream and yoghurt'),
  ('Eggs',            'eggs',            'Free range, organic and specialty eggs'),
  ('Livestock',       'livestock',       'Beef, pork, lamb and game meat'),
  ('Vegetables',      'vegetables',      'Fresh seasonal vegetables'),
  ('Fruit',           'fruit',           'Fresh seasonal fruit'),
  ('Grains & Pulses', 'grains-pulses',   'Wheat, oats, lentils and dried beans'),
  ('Honey & Preserves', 'honey-preserves', 'Raw honey, jams and chutneys'),
  ('Herbs & Flowers', 'herbs-flowers',   'Fresh cut herbs and edible flowers'),
  ('Bread & Baked',   'bread-baked',     'Sourdough, loaves and pastries')
ON CONFLICT (slug) DO NOTHING;

-- Children under Poultry
INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Chicken', 'chicken', 'Fresh whole and jointed chicken', id FROM categories WHERE slug = 'poultry'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Turkey', 'turkey', 'Whole and jointed turkey', id FROM categories WHERE slug = 'poultry'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Duck', 'duck', 'Duck and duck eggs', id FROM categories WHERE slug = 'poultry'
ON CONFLICT (slug) DO NOTHING;

-- Children under Dairy
INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Milk & Cream', 'milk-cream', 'Fresh whole, semi and skimmed milk', id FROM categories WHERE slug = 'dairy'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Cheese', 'cheese', 'Artisan and farmhouse cheeses', id FROM categories WHERE slug = 'dairy'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Butter & Spreads', 'butter-spreads', 'Cultured butter and farm spreads', id FROM categories WHERE slug = 'dairy'
ON CONFLICT (slug) DO NOTHING;

-- Children under Livestock
INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Beef', 'beef', 'Grass-fed beef cuts and mince', id FROM categories WHERE slug = 'livestock'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Pork', 'pork', 'Free range pork and cured meats', id FROM categories WHERE slug = 'livestock'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Lamb', 'lamb', 'Hogget, lamb and mutton', id FROM categories WHERE slug = 'livestock'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Game', 'game', 'Venison, rabbit and wild game', id FROM categories WHERE slug = 'livestock'
ON CONFLICT (slug) DO NOTHING;

-- Children under Vegetables
INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Root Vegetables', 'root-vegetables', 'Carrots, parsnips, turnips and beetroot', id FROM categories WHERE slug = 'vegetables'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Leafy Greens', 'leafy-greens', 'Kale, spinach, lettuce and chard', id FROM categories WHERE slug = 'vegetables'
ON CONFLICT (slug) DO NOTHING;

INSERT INTO categories (name, slug, description, parent_id)
SELECT 'Brassicas', 'brassicas', 'Cabbage, broccoli and cauliflower', id FROM categories WHERE slug = 'vegetables'
ON CONFLICT (slug) DO NOTHING;
