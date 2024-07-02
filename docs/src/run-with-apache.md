## Using with Apache

You can run Martin behind Apache "kind of" proxy, so you can use HTTPs with it. Here is an example of the configuration file that runs Martin with Apache.

First you have to setup a virtual host that is working on the port 443.

### Enable necessary modules

Ensure the required modules are enabled:

```bash

sudo a2enmod proxy
sudo a2enmod proxy_http
sudo a2enmod headers
sudo a2enmod rewrite

```

### Modify your VHOST configuration

Open your VHOST configuration file for the domaine you're using, mydomain.tld :

```bash

sudo nano /etc/apache2/sites-available/mydomain.tld.conf

```

### Update the configuration

```apache

<VirtualHost *:443>
    ServerName mydomain.tld
    ServerAdmin webmaster@localhost
    DocumentRoot /var/www/mydomain
    ProxyPreserveHost On
    
    RewriteEngine on
    RewriteCond %{REQUEST_URI} ^/tiles/(.*)$
    RewriteRule ^/tiles/(.*)$ http://localhost:3000/tiles/$1 [P,L]
    
    <IfModule mod_headers.c>
        RequestHeader set X-Forwarded-Proto "https"
    </IfModule>

    ProxyPass / http://localhost:3000/
    ProxyPassReverse / http://localhost:3000/
</VirtualHost>

```

### Check Configuration:  Verify the Apache configuration for syntax errors

```bash

sudo apache2ctl configtest

```

### Restart Apache: If the configuration is correct, restart Apache to apply the changes

```bash

sudo systemctl restart apache2

```
