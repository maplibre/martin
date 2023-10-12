create extension postgis_raster;
alter DATABASE db SET postgis.enable_outdb_rasters = true;
alter DATABASE db set postgis.gdal_enabled_drivers To  'GTiff PNG JPEG';
SELECT pg_reload_conf();